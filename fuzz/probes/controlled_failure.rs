//! Explicit opt-in tool proof; never part of production-target campaigns.
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    const MARKER: &[u8] = b"BK-FUZZ-CONTROL";
    assert!(
        !data.windows(MARKER.len()).any(|bytes| bytes == MARKER),
        "controlled fuzz harness failure probe"
    );
});
