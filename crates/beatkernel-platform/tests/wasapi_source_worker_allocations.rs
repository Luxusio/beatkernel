//! Exact production WASAPI worker/state/history code in a separate allocator
//! test binary. Native COM, OS scheduling and acoustic behavior are not tested.
#![allow(dead_code, unused_imports)]

use beatkernel_platform::audio::*;

#[path = "../src/audio/channel_remix.rs"]
mod channel_remix;
#[path = "../src/audio/negotiation.rs"]
mod negotiation;
#[path = "../src/audio/output_state.rs"]
mod output_state;
#[path = "../src/audio/source_history_retirement_mailbox.rs"]
mod source_history_retirement_mailbox;
#[path = "../src/audio/source_playback_publication.rs"]
mod source_playback_publication;
#[path = "../src/audio/wasapi_source_worker.rs"]
mod wasapi_source_worker;
use output_state::NativeOutputState;

// Source inclusion preserves private production history access. Its existing
// output-state fixture supplies the sole System allocator in this binary;
// production library unsafe-code policy and API visibility stay unchanged.
mod audio {
    pub(crate) use crate::negotiation;
    pub(crate) use crate::output_state::output_state_fixtures::{
        count_heap_calls, fail_allocation,
    };
    pub(crate) use crate::output_state::NativeOutputState;
    pub(crate) use crate::source_history_retirement_mailbox;
    pub(crate) use crate::source_playback_publication;
    pub use beatkernel_platform::audio::*;
}

#[test]
fn wasapi_source_worker_external_allocation_calibration_and_exact_steps() {
    use audio::count_heap_calls;
    let (allocation, calls) =
        count_heap_calls(|| Vec::<u64>::with_capacity(std::hint::black_box(128)));
    assert!(calls > 0, "calibration must observe a real allocation");
    drop(allocation);

    let mut allocation = Vec::<u64>::with_capacity(1);
    let ((), calls) = count_heap_calls(|| allocation.reserve_exact(std::hint::black_box(128)));
    assert!(calls > 0, "calibration must observe a real reallocation");
    drop(allocation);

    let allocation = Vec::<u64>::with_capacity(128);
    let ((), calls) = count_heap_calls(|| drop(std::hint::black_box(allocation)));
    assert!(calls > 0, "calibration must observe a real deallocation");

    wasapi_source_worker::fixtures::run_allocation_boundaries();
}
