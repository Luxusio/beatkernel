// Original BeatKernel MIT bridge source. SDK headers remain caller-supplied;
// SDK-combined artifacts follow the project's separate distribution contract.
// This file forwards only driver control. No callbacks, buffers or start/stop.
#if !defined(_WIN32) || !defined(_MSC_VER)
#error "The ASIO control bridge requires Windows with an MSVC-compatible C++ ABI"
#endif
#include <windows.h>
#include <objbase.h>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <memory>
#include <new>
#include <type_traits>
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
namespace {
constexpr std::int32_t Com = 1, Asio = 2, Win32 = 3, InitBoolean = 4, Bridge = 5;
constexpr std::int32_t BadArgument = 1, WrongThread = 2, BadString = 3,
    NativeException = 4, Allocation = 5;
BkAsioStatus ok() noexcept { return {0, 0}; }
BkAsioStatus asio_result(ASIOError code) noexcept {
    return code == ASE_OK ? ok() : BkAsioStatus{Asio, static_cast<std::int32_t>(code)};
}
struct Control {
    DWORD owner = GetCurrentThreadId();
    IASIO* driver = nullptr;
    bool apartment = false;
    BkAsioStatus cleanup() noexcept {
        BkAsioStatus result = ok();
        if (driver) {
            IASIO* release = driver;
            driver = nullptr;
            try { release->Release(); }
            catch (...) { result = {Bridge, NativeException}; }
        }
        if (apartment) { apartment = false; CoUninitialize(); }
        return result;
    }
    ~Control() noexcept { cleanup(); }
};
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
extern "C" BkAsioStatus bk_asio_close(void* raw) noexcept {
    if (!raw) return {Bridge, BadArgument};
    auto* control = static_cast<Control*>(raw);
    if (control->owner != GetCurrentThreadId()) return {Bridge, WrongThread};
    std::unique_ptr<Control> owned(control);
    return owned->cleanup();
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
