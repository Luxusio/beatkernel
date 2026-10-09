#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    beatkernel_layer_fuzz::codecs::check_replay(data);
});
