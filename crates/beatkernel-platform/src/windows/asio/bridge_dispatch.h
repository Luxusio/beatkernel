// Original BeatKernel MIT callback dispatch. No SDK types cross this seam.
#ifndef BEATKERNEL_ASIO_BRIDGE_DISPATCH_H
#define BEATKERNEL_ASIO_BRIDGE_DISPATCH_H
#include <cstdint>
#include <utility>

namespace beatkernel_asio {
// Ops and the late supplier are bounded and noexcept. The caller retains its
// admitted native reader throughout this function; enter/leave serialize render.
template<class Ops, class Supplier>
void dispatch_buffer(Ops& ops, std::int32_t index, Supplier&& supply_event) noexcept {
    if (!ops.ready()) return;
    if (index != 0 && index != 1) { ops.invalid_index(); return; }
    if (!ops.enter()) { ops.reentrant(); return; }
    const auto disposition = ops.preflight(index);
    if (disposition == 1) {
        // Retirement is permission to stop consuming Source, not a render error.
        // Silence each selected half on every callback, including the other half.
        ops.silence(index);
        ops.leave();
        return;
    }
    if (disposition != 0) {
        ops.render_failed(disposition);
    } else {
        const auto event = std::forward<Supplier>(supply_event)();
        ops.publish(event);
        if (!ops.fatal()) {
            const auto result = ops.render(index, event);
            if (result != 0) ops.render_failed(result);
        }
    }
    if (ops.fatal()) ops.silence(index);
    ops.leave();
}
}
#endif
