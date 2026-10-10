//! Standalone wasm32 regression fixture using the actual native publisher.
//!
//! Compile from the repository root:
//! rustc --crate-type cdylib --target wasm32-unknown-unknown --edition 2021 \
//!   app/tests/fixtures/wasm_native_publication.rs -o /tmp/native-publication.wasm
//! Run:
//! BEATKERNEL_NATIVE_PUBLICATION_WASM=/tmp/native-publication.wasm \
//!   node --test app/tests/wasm_native_publication.test.mjs

#[path = "../../src/native_publication.rs"]
mod native_publication;

use std::{io, path::Path};

#[no_mangle]
pub extern "C" fn ordinary_refusal() -> u32 {
    match native_publication::publish_new(Path::new("ordinary-record.bkr"), b"complete record") {
        Err(error) if error.kind() == io::ErrorKind::Unsupported => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn reserved_refusal() -> u32 {
    match native_publication::publish_new(Path::new("1234ABCD.0EF"), b"complete record") {
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => 1,
        _ => 0,
    }
}
