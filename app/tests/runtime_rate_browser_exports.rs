//! Compile-time checks of the actual WASM owner exports.
//! These are typing checks; host execution is not browser acceptance.
#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use beatkernel_bms_runtime::{
    browser::{BrowserPreparationJob, BrowserPrepared},
    browser_game::BrowserGame,
    browser_local_game::BrowserLocalGame,
};
use wasm_bindgen::JsValue;

macro_rules! retained_exports {
    ($owner:ty) => {{
        let _: fn(&mut $owner) -> Result<Vec<u32>, JsValue> = |g| g.retained_setup_words();
        let _: fn(&mut $owner) -> Result<u32, JsValue> = |g| g.retained_cue_count();
        let _: fn(&mut $owner, u32) -> Result<Vec<u32>, JsValue> =
            |g, index| g.retained_cue_words(index);
        let _: fn(&mut $owner) -> Result<Vec<u32>, JsValue> = |g| g.retained_capability_words();
        let _: fn(&mut $owner, i64, i64) -> Result<(), JsValue> =
            |g, output_ns, host_ns| g.retained_observe_presentation(output_ns, host_ns);
        let _: fn(&mut $owner) -> Result<Option<u64>, JsValue> = |g| g.retained_history_pin();
        let _: fn(&mut $owner, u64, i64, u64) -> Result<Vec<u32>, JsValue> =
            |g, id, n, d| g.retained_prepare_rate(id, n, d);
        let _: fn(&mut $owner, u64, i64, i64, bool) -> Result<(), JsValue> =
            |g, id, start, end, repeat| g.retained_prepare_region(id, start, end, repeat);
        let _: fn(&mut $owner, u64) -> Result<(), JsValue> =
            |g, id| g.retained_prepare_disable_loop(id);
        let _: fn(&mut $owner, u64) -> Result<Vec<u32>, JsValue> =
            |g, id| g.retained_prepare_original_restart(id);
        let _: fn(&mut $owner, u64, bool) -> Result<(), JsValue> =
            |g, id, paused| g.retained_prepare_pause(id, paused);
        let _: fn(&mut $owner, u32, u64, bool, String) -> Result<(), JsValue> =
            |g, kind, id, accepted, error| g.retained_admission(kind, id, accepted, error);
        let _: fn(&mut $owner, Vec<u32>) -> Result<(), JsValue> =
            |g, words| g.retained_history(words);
        let _: fn(&mut $owner, Vec<u32>) -> Result<(), JsValue> =
            |g, words| g.retained_event(words);
        let _: fn(&mut $owner, i64, i64) -> Result<Option<u64>, JsValue> =
            |g, host, audio| g.retained_service(host, audio);
        let _: fn(&mut $owner, u64) -> Result<(), JsValue> =
            |g, serial| g.retained_event_ack(serial);
        let _: fn(&mut $owner) -> Result<Vec<u32>, JsValue> = |g| g.retained_response_words();
    }};
}

#[test]
fn solo_actual_wasm_exports_preserve_integer_identity_and_typed_fallibility() {
    retained_exports!(BrowserGame);
    let _: fn(&mut BrowserGame) -> Result<Option<Vec<u8>>, JsValue> = |g| g.take_retired_replay();
}

#[test]
fn local_actual_wasm_exports_use_the_same_control_protocol_and_explicit_member_archive() {
    retained_exports!(BrowserLocalGame);
    let _: fn(&mut BrowserLocalGame, u32) -> Result<Option<Vec<u8>>, JsValue> =
        |g, player| g.take_retired_replay(player);
}

#[test]
fn retained_opt_in_preserves_the_owned_preparation_handoff_interface() {
    let _: fn(&mut BrowserPreparationJob) -> Result<(), JsValue> =
        |job| job.enable_retained_practice();
    let _: fn(&BrowserPrepared) -> bool = |prepared| prepared.retained_practice();
}
