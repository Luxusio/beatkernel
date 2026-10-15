// The real production helper; no SDK-shaped headers or native substitutes.
#include "../src/windows/asio/bridge_dispatch.h"
#include <array>
#include <cassert>
#include <cstdint>
#include <vector>
namespace {
struct Event { std::int32_t index; std::uint64_t position; };
enum Call { Ready, InvalidIndex, Enter, Reentrant, Preflight, Query, Parse,
    Publish, Fatal, Render, RenderFailed, Silence, Leave };
struct Driver {
    // Two live native halves of two heterogeneous-width channels, three frames.
    std::array<std::array<unsigned char, 6>, 2> int16{};
    std::array<std::array<unsigned char, 12>, 2> float32{};
    std::vector<Call> calls;
    bool available = true, admitted = true, failed = false;
    std::int32_t disposition = 0, render_result = 0, error = 0;
    unsigned queries = 0, parses = 0, publications = 0, renders = 0;
    unsigned source_admissions = 0, leaves = 0;
    Event published{};
    Driver() { refill(); calls.reserve(128); }
    void refill() noexcept {
        for (auto &half : int16) half.fill(0xa5);
        for (auto &half : float32) half.fill(0x5a);
    }
    bool ready() noexcept { calls.push_back(Ready); return available; }
    void invalid_index() noexcept { calls.push_back(InvalidIndex); }
    bool enter() noexcept { calls.push_back(Enter); return admitted; }
    void reentrant() noexcept { calls.push_back(Reentrant); }
    std::int32_t preflight(std::int32_t) noexcept {
        calls.push_back(Preflight); return disposition;
    }
    void silence(std::int32_t index) noexcept {
        calls.push_back(Silence); int16[index].fill(0); float32[index].fill(0);
    }
    void publish(const Event &event) noexcept {
        calls.push_back(Publish); ++publications; published = event;
    }
    bool fatal() noexcept { calls.push_back(Fatal); return failed; }
    std::int32_t render(std::int32_t index, const Event &event) noexcept {
        calls.push_back(Render); ++renders; assert(event.index == index);
        if (!render_result) ++source_admissions;
        return render_result;
    }
    void render_failed(std::int32_t code) noexcept {
        calls.push_back(RenderFailed); error = code; failed = true;
    }
    void leave() noexcept { calls.push_back(Leave); ++leaves; }
    bool zero(unsigned index) const noexcept {
        for (auto b : int16[index]) if (b) return false;
        for (auto b : float32[index]) if (b) return false;
        return true;
    }
    bool untouched(unsigned index) const noexcept {
        for (auto b : int16[index]) if (b != 0xa5) return false;
        for (auto b : float32[index]) if (b != 0x5a) return false;
        return true;
    }
    void dispatch(std::int32_t index) noexcept {
        beatkernel_asio::dispatch_buffer(*this, index, [&, index]() noexcept {
            calls.push_back(Query); ++queries;
            calls.push_back(Parse); ++parses;
            return Event{index, 0xfedcba9876543210ULL};
        });
    }
};
void retired_both_halves_repeatedly_before_all_evidence_work() {
    Driver d; d.disposition = 1; d.failed = true;
    for (unsigned i = 0; i < 8; ++i) {
        d.refill(); d.calls.clear(); d.dispatch(i % 2);
        assert(d.zero(i % 2) && d.untouched(1 - i % 2));
        assert((d.calls == std::vector<Call>{Ready, Enter, Preflight, Silence, Leave}));
    }
    assert(d.leaves == 8 && d.error == 0);
    assert(d.queries == 0 && d.parses == 0 && d.publications == 0);
    assert(d.renders == 0 && d.source_admissions == 0);
}
void continue_retains_original_event_and_raw_before_delivery() {
    Driver d; d.dispatch(1);
    assert((d.calls == std::vector<Call>{Ready, Enter, Preflight, Query, Parse,
        Publish, Fatal, Render, Fatal, Leave}));
    assert(d.published.index == 1 && d.published.position == 0xfedcba9876543210ULL);
    assert(d.queries == 1 && d.parses == 1 && d.publications == 1);
    assert(d.renders == 1 && d.source_admissions == 1 && d.leaves == 1);
    assert(d.untouched(0) && d.untouched(1) && d.error == 0);
}
void admission_refusals_never_touch_planes_or_query() {
    for (auto index : {-1, 2}) {
        Driver d; d.dispatch(index);
        assert((d.calls == std::vector<Call>{Ready, InvalidIndex}));
        assert(d.untouched(0) && d.untouched(1) && d.queries == 0 && d.leaves == 0);
    }
    Driver cold; cold.available = false; cold.dispatch(0);
    assert((cold.calls == std::vector<Call>{Ready}));
    assert(cold.untouched(0) && cold.untouched(1));
    Driver busy; busy.admitted = false; busy.dispatch(0);
    assert((busy.calls == std::vector<Call>{Ready, Enter, Reentrant}));
    assert(busy.queries == 0 && busy.leaves == 0 && busy.untouched(0) && busy.untouched(1));
}
void preflight_error_does_not_query_publish_render_or_admit() {
    Driver d; d.disposition = 3; d.dispatch(0);
    assert((d.calls == std::vector<Call>{Ready, Enter, Preflight, RenderFailed,
        Fatal, Silence, Leave}));
    assert(d.error == 3 && d.failed && d.zero(0) && d.untouched(1));
    assert(d.queries == 0 && d.publications == 0 && d.renders == 0);
    assert(d.source_admissions == 0 && d.leaves == 1);
}
void existing_fatal_keeps_raw_diagnostic_but_never_delivers() {
    Driver d; d.failed = true; d.dispatch(1);
    assert((d.calls == std::vector<Call>{Ready, Enter, Preflight, Query, Parse,
        Publish, Fatal, Fatal, Silence, Leave}));
    assert(d.publications == 1 && d.queries == 1 && d.renders == 0);
    assert(d.source_admissions == 0 && d.zero(1) && d.untouched(0));
    assert(d.error == 0 && d.leaves == 1);
}
void render_error_is_preserved_and_selected_half_silenced() {
    Driver d; d.render_result = 2; d.dispatch(0);
    assert((d.calls == std::vector<Call>{Ready, Enter, Preflight, Query, Parse,
        Publish, Fatal, Render, RenderFailed, Fatal, Silence, Leave}));
    assert(d.error == 2 && d.failed && d.renders == 1 && d.source_admissions == 0);
    assert(d.publications == 1 && d.leaves == 1 && d.zero(0) && d.untouched(1));
}
}
int main() {
    retired_both_halves_repeatedly_before_all_evidence_work();
    continue_retains_original_event_and_raw_before_delivery();
    admission_refusals_never_touch_planes_or_query();
    preflight_error_does_not_query_publish_render_or_admit();
    existing_fatal_keeps_raw_diagnostic_but_never_delivers();
    render_error_is_preserved_and_selected_half_silenced();
}
