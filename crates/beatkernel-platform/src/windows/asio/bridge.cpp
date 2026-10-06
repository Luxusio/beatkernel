// Original BeatKernel MIT bridge source. SDK headers remain caller-supplied;
// SDK-combined artifacts follow the project's separate distribution contract.
// Driver control and owned SDK buffers; SDK layouts do not cross the C ABI.
#if !defined(_WIN32) || !defined(_MSC_VER)
#error "The ASIO control bridge requires Windows with an MSVC-compatible C++ ABI"
#endif
#include <windows.h>
#include <objbase.h>
#include <cstdint>
#include <cstddef>
#include <cstring>
#include <cmath>
#include <memory>
#include <new>
#include <type_traits>
#include <atomic>
#include <limits>
#include <thread>
#include "iasiodrv.h"

static_assert(sizeof(long) == 4, "IASIO Windows long must be 32-bit");
static_assert(std::is_same<ASIOSampleRate, double>::value, "ASIO rate must be IEEE double");
static_assert(sizeof(ASIOChannelInfo::name) == 32, "Unexpected SDK channel name extent");

// Project-owned fixed-width C ABI. SDK structures never cross into Rust.
struct BkAsioStatus { std::int32_t domain; std::int32_t code; };
struct BkAsioChannel {
    std::int32_t channel, input, active, group, sample_type;
    std::uint8_t name[32];
};
static_assert(sizeof(BkAsioStatus) == 8, "Bridge status ABI mismatch");
static_assert(sizeof(BkAsioChannel) == 52, "Bridge channel ABI mismatch");
struct BkAsioClock {
    std::int32_t index, channel, group, current;
    std::uint8_t name[32];
};
static_assert(sizeof(BkAsioClock) == 48, "Bridge clock ABI mismatch");
struct BkAsioOutput {
    std::int32_t channel, sample_type;
    std::uint32_t width;
    void* buffer0;
    void* buffer1;
};
struct BkAsioEvent {
    std::int32_t index, direct;
    std::uint32_t flags;
    std::uint64_t position, system_ns;
    double rate;
};
struct BkAsioDiagnostics {
    std::uint32_t flags;
    std::int32_t render_error, clock_available, reserved;
    BkAsioEvent event;
};
using RenderCallback = std::int32_t (__cdecl*)(void*, std::int32_t, const BkAsioEvent*);
static_assert(sizeof(BkAsioOutput) == (sizeof(void*) == 8 ? 32 : 20), "Output ABI");
static_assert(sizeof(BkAsioEvent) == 40 && sizeof(BkAsioDiagnostics) == 56, "Clock ABI");
static_assert(std::atomic<std::uint64_t>::is_always_lock_free, "RT uint64 atomic");
static_assert(std::atomic<std::uint32_t>::is_always_lock_free, "RT uint32 atomic");
static_assert(std::atomic<void*>::is_always_lock_free, "RT pointer atomic");
static_assert(std::atomic<bool>::is_always_lock_free, "RT bool atomic");
static_assert(std::atomic<std::int32_t>::is_always_lock_free, "RT int32 atomic");
namespace {
constexpr std::int32_t Com = 1, Asio = 2, Win32 = 3, InitBoolean = 4, Bridge = 5;
constexpr std::int32_t BadArgument = 1, WrongThread = 2, BadString = 3,
    NativeException = 4, Allocation = 5;
BkAsioStatus ok() noexcept { return {0, 0}; }
BkAsioStatus asio_result(ASIOError code) noexcept {
    return code == ASE_OK ? ok() : BkAsioStatus{Asio, static_cast<std::int32_t>(code)};
}
struct Control;
std::atomic<Control*> active{nullptr};
std::atomic<Control*> reservation{nullptr};
std::atomic<std::uint64_t> readers{0};
constexpr std::uint32_t Reset = 1, Resync = 2, Latencies = 4, Rate = 8,
    BufferSize = 16, Overload = 32, Reentrant = 64, InvalidIndex = 128,
    RenderFailed = 256, ClockExhausted = 512, MalformedTime = 1024;
constexpr std::uint32_t Fatal = Reset | Resync | Latencies | Rate | BufferSize |
    Reentrant | InvalidIndex | RenderFailed | MalformedTime;
struct Control {
    DWORD owner = GetCurrentThreadId();
    IASIO* driver = nullptr;
    bool apartment = false, reserved = false, create_attempted = false,
        start_attempted = false, prepared = false;
    std::atomic<bool> ready{false};
    std::atomic_flag rendering = ATOMIC_FLAG_INIT;
    ASIOBufferInfo rows[32]{};
    BkAsioOutput outputs[32]{};
    ASIOCallbacks callbacks{};
    std::int32_t count = 0, frames = 0;
    double configured_rate = 0;
    RenderCallback render = nullptr;
    void* context = nullptr;
    std::atomic<std::uint32_t> faults{0};
    std::atomic<std::int32_t> render_error{0};
    std::atomic<std::uint64_t> version{0}, position{0}, system_ns{0}, rate_bits{0};
    std::atomic<std::uint32_t> event_flags{0};
    std::atomic<std::int32_t> event_index{0}, event_direct{0};
    void detach() noexcept {
        ready.store(false, std::memory_order_seq_cst);
        if (reserved) {
            active.exchange(nullptr, std::memory_order_seq_cst);
            while (readers.load(std::memory_order_seq_cst) != 0) std::this_thread::yield();
        }
    }
    BkAsioStatus cleanup() noexcept {
        detach();
        BkAsioStatus result = ok();
        auto retain = [&](BkAsioStatus next) { if (!result.domain) result = next; };
        if (driver) {
            if (start_attempted) {
                start_attempted = false;
                try { retain(asio_result(driver->stop())); }
                catch (...) { retain({Bridge, NativeException}); }
            }
            if (create_attempted) {
                create_attempted = false;
                try { retain(asio_result(driver->disposeBuffers())); }
                catch (...) { retain({Bridge, NativeException}); }
            }
            IASIO* release = driver;
            driver = nullptr;
            try { release->Release(); }
            catch (...) { retain({Bridge, NativeException}); }
        }
        if (reserved) { reservation.store(nullptr, std::memory_order_seq_cst); reserved = false; }
        if (apartment) { apartment = false; CoUninitialize(); }
        return result;
    }
    ~Control() noexcept { cleanup(); }
};
// Increment before pointer load: detach can never free an admitted callback's owner.
struct Admission {
    Control* control = nullptr;
    bool entered = false;
    Admission() noexcept {
        auto value = readers.load(std::memory_order_seq_cst);
        while (value != std::numeric_limits<std::uint64_t>::max()) {
            if (readers.compare_exchange_weak(value, value + 1, std::memory_order_seq_cst)) {
                entered = true;
                control = active.load(std::memory_order_seq_cst);
                return;
            }
        }
    }
    ~Admission() { if (entered) readers.fetch_sub(1, std::memory_order_seq_cst); }
};
template<class Native64> std::uint64_t native64(const Native64& value) noexcept {
    if constexpr (std::is_integral<Native64>::value) {
        return static_cast<std::uint64_t>(value);
    } else {
        return (static_cast<std::uint64_t>(static_cast<std::uint32_t>(value.hi)) << 32)
            | static_cast<std::uint32_t>(value.lo);
    }
}
void publish(Control* c, const BkAsioEvent& event) noexcept {
    const auto version = c->version.load(std::memory_order_seq_cst);
    if (version > std::numeric_limits<std::uint64_t>::max() - 2) {
        c->faults.fetch_or(ClockExhausted); return;
    }
    c->version.store(version + 1, std::memory_order_seq_cst);
    c->event_index.store(event.index, std::memory_order_seq_cst);
    c->event_direct.store(event.direct, std::memory_order_seq_cst);
    c->event_flags.store(event.flags, std::memory_order_seq_cst);
    c->position.store(event.position, std::memory_order_seq_cst);
    c->system_ns.store(event.system_ns, std::memory_order_seq_cst);
    std::uint64_t bits; std::memcpy(&bits, &event.rate, sizeof(bits));
    c->rate_bits.store(bits, std::memory_order_seq_cst);
    c->version.store(version + 2, std::memory_order_seq_cst);
}
void buffer_callback(Control* c, const BkAsioEvent& event) noexcept {
    if (!c->ready.load(std::memory_order_seq_cst)) return;
    if (event.index != 0 && event.index != 1) { c->faults.fetch_or(InvalidIndex); return; }
    if (c->rendering.test_and_set(std::memory_order_acquire)) {
        c->faults.fetch_or(Reentrant); return;
    }
    publish(c, event);
    if (!(c->faults.load() & Fatal)) {
        const auto result = c->render(c->context, event.index, &event);
        if (result) {
            std::int32_t empty = 0; c->render_error.compare_exchange_strong(empty, result);
            c->faults.fetch_or(RenderFailed);
        }
    }
    if (c->faults.load() & Fatal) {
        for (int i = 0; i < c->count; ++i)
            std::memset(c->rows[i].buffers[event.index], 0,
                static_cast<std::size_t>(c->frames) * c->outputs[i].width);
    }
    c->rendering.clear(std::memory_order_release);
}
void legacy_switch(long index, ASIOBool direct) {
    Admission admission; auto* c = admission.control; if (!c) return;
    BkAsioEvent event{}; event.index = static_cast<std::int32_t>(index); event.direct = direct;
    ASIOSamples position{}; ASIOTimeStamp time{};
    try {
        if (c->driver->getSamplePosition(&position, &time) == ASE_OK) {
            event.flags = 3; event.position = native64(position); event.system_ns = native64(time);
        }
    } catch (...) { c->faults.fetch_or(MalformedTime); }
    buffer_callback(c, event);
}
ASIOTime* time_switch(ASIOTime* time, long index, ASIOBool direct) {
    Admission admission; auto* c = admission.control; if (!c) return time;
    BkAsioEvent event{}; event.index = static_cast<std::int32_t>(index); event.direct = direct;
    if (!time) c->faults.fetch_or(MalformedTime);
    else {
        event.flags = static_cast<std::uint32_t>(time->timeInfo.flags);
        if (event.flags & kClockSourceChanged) c->faults.fetch_or(Resync);
        if (event.flags & kSampleRateChanged) c->faults.fetch_or(Rate);
        if (event.flags & kSamplePositionValid) event.position = native64(time->timeInfo.samplePosition);
        if (event.flags & kSystemTimeValid) event.system_ns = native64(time->timeInfo.systemTime);
        if (event.flags & kSampleRateValid) {
            event.rate = time->timeInfo.sampleRate;
            if (!std::isfinite(event.rate) || event.rate <= 0) c->faults.fetch_or(MalformedTime);
            else if (event.rate != c->configured_rate) c->faults.fetch_or(Rate);
        }
    }
    buffer_callback(c, event); return time;
}
void rate_changed(ASIOSampleRate rate) {
    Admission admission; auto* c = admission.control; if (!c) return;
    if (!std::isfinite(rate) || rate <= 0 || rate != c->configured_rate) c->faults.fetch_or(Rate);
}
bool supported(long selector) noexcept {
    return selector == kAsioEngineVersion || selector == kAsioResetRequest ||
        selector == kAsioResyncRequest || selector == kAsioLatenciesChanged ||
        selector == kAsioSupportsTimeInfo || selector == kAsioSupportsTimeCode ||
        selector == kAsioOverload;
}
long message(long selector, long value, void*, double*) {
    Admission admission; auto* c = admission.control; if (!c) return 0;
    if (selector == kAsioSelectorSupported) return supported(value) ? 1 : 0;
    switch (selector) {
        case kAsioEngineVersion: return 2;
        case kAsioSupportsTimeInfo: return 1;
        case kAsioSupportsTimeCode: return 0;
        case kAsioResetRequest: c->faults.fetch_or(Reset); return 1;
        case kAsioResyncRequest: c->faults.fetch_or(Resync); return 1;
        case kAsioLatenciesChanged: c->faults.fetch_or(Latencies); return 1;
        case kAsioBufferSizeChange: c->faults.fetch_or(BufferSize); return 0;
        case kAsioOverload: c->faults.fetch_or(Overload); return 1;
        default: return 0;
    }
}
std::uint32_t pcm_width(std::int32_t type) noexcept {
    switch (type) {
        case 0: case 16: return 2;
        case 1: case 17: return 3;
        case 4: case 20: return 8;
        case 2: case 3: case 8: case 9: case 10: case 11:
        case 18: case 19: case 24: case 25: case 26: case 27: return 4;
        default: return 0;
    }
}
template<class Fn> BkAsioStatus guarded(void* raw, Fn&& operation) noexcept {
    if (!raw) return {Bridge, BadArgument};
    auto* control = static_cast<Control*>(raw);
    if (control->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    if (!control->driver) return {Bridge, BadArgument};
    try { return operation(control->driver); }
    catch (...) { return {Bridge, NativeException}; }
}
}

extern "C" BkAsioStatus bk_asio_open(const std::uint16_t* clsid_text,
    std::uintptr_t host_window, void** output) noexcept {
    if (!clsid_text || !output) return {Bridge, BadArgument};
    *output = nullptr;
    if (host_window && !IsWindow(reinterpret_cast<HWND>(host_window)))
        return {Win32, ERROR_INVALID_WINDOW_HANDLE};
    if (host_window && GetWindowThreadProcessId(reinterpret_cast<HWND>(host_window), nullptr)
            != GetCurrentThreadId())
        return {Bridge, WrongThread};
    std::unique_ptr<Control> control(new (std::nothrow) Control);
    if (!control) return {Bridge, Allocation};
    try {
        // Both S_OK and S_FALSE require a matching CoUninitialize. Changed-mode
        // and every other failed HRESULT return unchanged, without balancing.
        HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
        if (FAILED(hr)) return {Com, static_cast<std::int32_t>(hr)};
        control->apartment = true;
        CLSID clsid{};
        hr = CLSIDFromString(reinterpret_cast<LPCOLESTR>(clsid_text), &clsid);
        if (FAILED(hr)) return {Com, static_cast<std::int32_t>(hr)};
        // ASIO's COM registration uses the selected CLSID as its IASIO IID.
        hr = CoCreateInstance(clsid, nullptr, CLSCTX_INPROC_SERVER, clsid,
            reinterpret_cast<void**>(&control->driver));
        if (FAILED(hr)) return {Com, static_cast<std::int32_t>(hr)};
        if (!control->driver) return {Bridge, BadArgument};
        const ASIOBool initialized = control->driver->init(reinterpret_cast<void*>(host_window));
        if (initialized == ASIOFalse) return {InitBoolean, static_cast<std::int32_t>(initialized)};
        *output = control.release();
        return ok();
    } catch (...) { return {Bridge, NativeException}; }
}
extern "C" BkAsioStatus bk_asio_close_with_retirement(void* raw, std::uint32_t* retired) noexcept {
    if (!retired) return {Bridge, BadArgument};
    *retired = 0;
    if (!raw) return {Bridge, BadArgument};
    auto* control = static_cast<Control*>(raw);
    if (control->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    std::unique_ptr<Control> owned(control);
    const auto result = owned->cleanup();
    // cleanup detached routing and drained admitted readers before driver calls.
    *retired = 1;
    return result;
}
extern "C" BkAsioStatus bk_asio_close(void* raw) noexcept {
    std::uint32_t retired = 0;
    return bk_asio_close_with_retirement(raw, &retired);
}
extern "C" BkAsioStatus bk_asio_channels(void* raw, std::int32_t* inputs, std::int32_t* outputs) noexcept {
    if (!inputs || !outputs) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) {
        long in = 0, out = 0;
        auto result = asio_result(driver->getChannels(&in, &out));
        if (!result.domain) { *inputs = static_cast<std::int32_t>(in); *outputs = static_cast<std::int32_t>(out); }
        return result;
    });
}
extern "C" BkAsioStatus bk_asio_buffer(void* raw, std::int32_t* minimum,
    std::int32_t* maximum, std::int32_t* preferred, std::int32_t* granularity) noexcept {
    if (!minimum || !maximum || !preferred || !granularity) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) {
        long min = 0, max = 0, pref = 0, gran = 0;
        auto result = asio_result(driver->getBufferSize(&min, &max, &pref, &gran));
        if (!result.domain) { *minimum = static_cast<std::int32_t>(min); *maximum = static_cast<std::int32_t>(max); *preferred = static_cast<std::int32_t>(pref); *granularity = static_cast<std::int32_t>(gran); }
        return result;
    });
}
extern "C" BkAsioStatus bk_asio_latencies(void* raw, std::int32_t* input, std::int32_t* output) noexcept {
    if (!input || !output) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) {
        long in = 0, out = 0;
        auto result = asio_result(driver->getLatencies(&in, &out));
        if (!result.domain) { *input = static_cast<std::int32_t>(in); *output = static_cast<std::int32_t>(out); }
        return result;
    });
}
extern "C" BkAsioStatus bk_asio_rate(void* raw, double* rate) noexcept {
    if (!rate) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) { return asio_result(driver->getSampleRate(rate)); });
}
extern "C" BkAsioStatus bk_asio_probe_rate(void* raw, double rate) noexcept {
    if (!std::isfinite(rate) || rate < 0) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) { return asio_result(driver->canSampleRate(rate)); });
}
extern "C" BkAsioStatus bk_asio_set_rate(void* raw, double rate) noexcept {
    if (!std::isfinite(rate) || rate < 0) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) { return asio_result(driver->setSampleRate(rate)); });
}
extern "C" BkAsioStatus bk_asio_channel(void* raw, std::int32_t index,
    std::int32_t input, BkAsioChannel* output) noexcept {
    if (!output || index < 0 || (input != 0 && input != 1)) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) {
        ASIOChannelInfo info{};
        info.channel = index;
        info.isInput = input ? ASIOTrue : ASIOFalse;
        std::memset(info.name, 0xff, sizeof(info.name));
        auto result = asio_result(driver->getChannelInfo(&info));
        if (result.domain) return result;
        if (!std::memchr(info.name, 0, sizeof(info.name))) return BkAsioStatus{Bridge, BadString};
        output->channel = static_cast<std::int32_t>(info.channel);
        output->input = static_cast<std::int32_t>(info.isInput);
        output->active = static_cast<std::int32_t>(info.isActive);
        output->group = static_cast<std::int32_t>(info.channelGroup);
        output->sample_type = static_cast<std::int32_t>(info.type);
        std::memcpy(output->name, info.name, sizeof(output->name));
        return ok();
    });
}
extern "C" BkAsioStatus bk_asio_control_panel(void* raw) noexcept {
    return guarded(raw, [&](IASIO* driver) { return asio_result(driver->controlPanel()); });
}

extern "C" BkAsioStatus bk_asio_clocks(void* raw, BkAsioClock* output,
    std::int32_t capacity, std::int32_t* count) noexcept {
    if (!output || !count || capacity < 1 || capacity > 4096) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) {
        auto clocks = std::unique_ptr<ASIOClockSource[]>(new (std::nothrow) ASIOClockSource[capacity]{});
        if (!clocks) return BkAsioStatus{Bridge, Allocation};
        for (int i = 0; i < capacity; ++i) std::memset(clocks[i].name, 0xff, sizeof(clocks[i].name));
        long available = capacity;
        auto result = asio_result(driver->getClockSources(clocks.get(), &available));
        if (result.domain) return result;
        *count = static_cast<std::int32_t>(available);
        // Required count can exceed capacity; no uninitialized rows are copied.
        if (available > capacity || available < 1) return ok();
        for (long i = 0; i < available; ++i)
            if (!std::memchr(clocks[i].name, 0, sizeof(clocks[i].name))) return BkAsioStatus{Bridge, BadString};
        for (long i = 0; i < available; ++i) {
            output[i].index = static_cast<std::int32_t>(clocks[i].index);
            output[i].channel = static_cast<std::int32_t>(clocks[i].associatedChannel);
            output[i].group = static_cast<std::int32_t>(clocks[i].associatedGroup);
            output[i].current = static_cast<std::int32_t>(clocks[i].isCurrentSource);
            std::memcpy(output[i].name, clocks[i].name, sizeof(output[i].name));
        }
        return ok();
    });
}
extern "C" BkAsioStatus bk_asio_select_clock(void* raw, std::int32_t index) noexcept {
    if (index < 0) return {Bridge, BadArgument};
    return guarded(raw, [&](IASIO* driver) { return asio_result(driver->setClockSource(index)); });
}

extern "C" BkAsioStatus bk_asio_prepare(void* raw, BkAsioOutput* outputs,
    std::int32_t count, std::int32_t frames, double rate,
    RenderCallback render, void* context) noexcept {
    if (!raw || !outputs || !render || !context || count < 1 || count > 32 ||
        frames <= 0 || !std::isfinite(rate) || rate <= 0) return {Bridge, BadArgument};
    auto* c = static_cast<Control*>(raw);
    if (c->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    if (!c->driver || c->reserved || c->create_attempted) return {Bridge, BadArgument};
    try {
        long inputs = 0, channels = 0;
        auto result = asio_result(c->driver->getChannels(&inputs, &channels));
        if (result.domain) return result;
        double actual = 0;
        result = asio_result(c->driver->getSampleRate(&actual));
        if (result.domain) return result;
        if (actual != rate) return {Bridge, BadArgument};
        for (int i = 0; i < count; ++i) {
            if (outputs[i].channel < 0 || outputs[i].channel >= channels ||
                !pcm_width(outputs[i].sample_type) ||
                outputs[i].width != pcm_width(outputs[i].sample_type) ||
                static_cast<std::size_t>(frames) >
                    static_cast<std::size_t>(std::numeric_limits<std::ptrdiff_t>::max()) / outputs[i].width)
                return {Bridge, BadArgument};
            for (int j = 0; j < i; ++j)
                if (outputs[i].channel == outputs[j].channel) return {Bridge, BadArgument};
            ASIOChannelInfo info{}; info.channel = outputs[i].channel; info.isInput = ASIOFalse;
            result = asio_result(c->driver->getChannelInfo(&info));
            if (result.domain) return result;
            if (info.channel != outputs[i].channel || info.isInput != ASIOFalse ||
                info.type != outputs[i].sample_type) return {Bridge, BadArgument};
            c->outputs[i] = outputs[i];
            c->rows[i].isInput = ASIOFalse; c->rows[i].channelNum = outputs[i].channel;
        }
        c->count = count; c->frames = frames; c->configured_rate = rate;
        c->render = render; c->context = context;
        c->callbacks.bufferSwitch = legacy_switch;
        c->callbacks.sampleRateDidChange = rate_changed;
        c->callbacks.asioMessage = message;
        c->callbacks.bufferSwitchTimeInfo = time_switch;
        Control* empty = nullptr;
        if (!reservation.compare_exchange_strong(empty, c, std::memory_order_seq_cst))
            return {Bridge, BadArgument};
        c->reserved = true;
        active.store(c, std::memory_order_seq_cst);
        c->create_attempted = true;
        result = asio_result(c->driver->createBuffers(c->rows, count, frames, &c->callbacks));
        if (result.domain) { c->detach(); return result; }
        // Validate all regions before touching any native memory. All halves of
        // all selected channels must be disjoint, not only the current half.
        std::uintptr_t starts[64]{}, ends[64]{};
        for (int i = 0; i < count; ++i) {
            ASIOChannelInfo info{}; info.channel = c->outputs[i].channel; info.isInput = ASIOFalse;
            result = asio_result(c->driver->getChannelInfo(&info));
            if (result.domain) { c->detach(); return result; }
            if (info.channel != c->outputs[i].channel || info.isInput != ASIOFalse ||
                info.type != c->outputs[i].sample_type || c->rows[i].isInput != ASIOFalse ||
                c->rows[i].channelNum != c->outputs[i].channel) {
                c->detach(); return {Bridge, BadArgument};
            }
            const auto bytes = static_cast<std::size_t>(frames) * c->outputs[i].width;
            for (int half = 0; half < 2; ++half) {
                const int n = i * 2 + half;
                starts[n] = reinterpret_cast<std::uintptr_t>(c->rows[i].buffers[half]);
                if (!starts[n] || bytes > std::numeric_limits<std::uintptr_t>::max() - starts[n]) {
                    c->detach(); return {Bridge, BadArgument};
                }
                ends[n] = starts[n] + bytes;
                for (int j = 0; j < n; ++j)
                    if (starts[n] < ends[j] && starts[j] < ends[n]) {
                        c->detach(); return {Bridge, BadArgument};
                    }
            }
        }
        result = asio_result(c->driver->getSampleRate(&actual));
        if (result.domain) { c->detach(); return result; }
        if (actual != rate || (c->faults.load() & Fatal)) {
            c->detach(); return {Bridge, BadArgument};
        }
        for (int i = 0; i < count; ++i) {
            c->outputs[i].buffer0 = c->rows[i].buffers[0];
            c->outputs[i].buffer1 = c->rows[i].buffers[1];
            outputs[i] = c->outputs[i];
            const auto bytes = static_cast<std::size_t>(frames) * c->outputs[i].width;
            std::memset(c->rows[i].buffers[0], 0, bytes);
            std::memset(c->rows[i].buffers[1], 0, bytes);
        }
        c->prepared = true;
        return ok();
    } catch (...) { c->detach(); return {Bridge, NativeException}; }
}
extern "C" BkAsioStatus bk_asio_start(void* raw) noexcept {
    if (!raw) return {Bridge, BadArgument};
    auto* c = static_cast<Control*>(raw);
    if (c->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    if (!c->driver || !c->prepared || c->start_attempted || (c->faults.load() & Fatal))
        return {Bridge, BadArgument};
    c->start_attempted = true;
    c->ready.store(true, std::memory_order_seq_cst);
    try {
        auto result = asio_result(c->driver->start());
        if (result.domain) c->detach();
        return result;
    } catch (...) { c->detach(); return {Bridge, NativeException}; }
}
extern "C" BkAsioStatus bk_asio_diagnostics(void* raw, BkAsioDiagnostics* output) noexcept {
    if (!raw || !output) return {Bridge, BadArgument};
    auto* c = static_cast<Control*>(raw);
    if (c->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    BkAsioDiagnostics result{};
    result.flags = c->faults.load(); result.render_error = c->render_error.load();
    if (!(result.flags & ClockExhausted)) {
        for (int attempt = 0; attempt < 3; ++attempt) {
            const auto before = c->version.load(std::memory_order_seq_cst);
            if (!before || (before & 1)) continue;
            BkAsioEvent event{};
            event.index = c->event_index.load(std::memory_order_seq_cst);
            event.direct = c->event_direct.load(std::memory_order_seq_cst);
            event.flags = c->event_flags.load(std::memory_order_seq_cst);
            event.position = c->position.load(std::memory_order_seq_cst);
            event.system_ns = c->system_ns.load(std::memory_order_seq_cst);
            const auto bits = c->rate_bits.load(std::memory_order_seq_cst);
            std::memcpy(&event.rate, &bits, sizeof(bits));
            if (before == c->version.load(std::memory_order_seq_cst)) {
                result.event = event;
                result.clock_available = (event.flags & 3) == 3 ? 1 : 0;
                break;
            }
        }
    }
    *output = result; return ok();
}
