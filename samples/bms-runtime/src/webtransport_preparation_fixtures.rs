use super::*;
use crate::{
    multiplayer_credentials::{CredentialReadPort, CredentialLoadError},
    multiplayer_start::StartRole,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

// Intentionally no Clone, Debug, Display or Error bounds on the reader failure.
struct Opaque(Arc<u8>);
struct Reader {
    reply: Option<Result<Vec<u8>, Opaque>>,
    calls: Vec<(usize, Vec<u8>)>,
}
impl CredentialReadPort for Reader {
    type Error = Opaque;
    fn read(&mut self, path: &Path) -> Result<Vec<u8>, Opaque> {
        let bytes = path.as_os_str().as_encoded_bytes();
        self.calls.push((bytes.as_ptr() as usize, bytes.to_vec()));
        self.reply.take().expect("unexpected CA acquisition")
    }
}
fn reader(reply: Option<Result<Vec<u8>, Opaque>>) -> Reader {
    Reader {
        reply,
        calls: vec![],
    }
}
fn options(role: StartRole) -> WebTransportOptions {
    WebTransportOptions {
        url: "https://relay.example:4433/rooms/Room_7-abc".into(),
        origin: "https://ui.example".into(),
        ca: PathBuf::from("original-ca.pem"),
        role,
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
mod supported {
    use super::*;
    fn refused(options: &WebTransportOptions) {
        let mut reader = reader(None);
        match prepare_webtransport(&mut reader, options) {
            Err(CredentialLoadError::Validation(error)) => {
                assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput)
            }
            _ => panic!("invalid complete metadata must refuse before any CA read"),
        }
        assert!(reader.calls.is_empty());
    }

    #[test]
    fn malformed_and_noncanonical_destinations_refuse_before_ca_acquisition() {
        for url in [
            "",
            "not a URL",
            "http://relay.example/rooms/a",
            "https://relay.example:0/rooms/a",
            "https://relay.example:65536/rooms/a",
            "https://user@relay.example/rooms/a",
            "https://user:secret@relay.example/rooms/a",
            "https://relay.example/rooms/a?x=1",
            "https://relay.example/rooms/a#fragment",
            "https://RELAY.example/rooms/a",
            "https://relay.example:443/rooms/a",
            "https://relay.example/rooms/",
            "https://relay.example/rooms/a/b",
            "https://relay.example/rooms/a%20b",
            "https://relay.example/rooms/a+b",
            "https://relay.example/other/a",
            "https://relay.example/rooms/../rooms/a",
        ] {
            let mut options = options(StartRole::Host);
            options.url = url.into();
            refused(&options);
        }
        for length in [1025, 4097] {
            let mut options = options(StartRole::Join);
            options.url = format!("https://relay.example/rooms/{}", "r".repeat(length));
            refused(&options);
        }
    }

    #[test]
    fn malformed_noncanonical_or_nonloopback_http_origins_refuse_before_reads() {
        for origin in [
            "",
            "null",
            "not an origin",
            "http://ui.example",
            "http://192.0.2.1",
            "https://ui.example/",
            "https://UI.example",
            "https://ui.example:443",
            "https://ui.example/path",
            "https://ui.example?x=1",
            "https://ui.example#part",
            "https://user@ui.example",
            "https://user:secret@ui.example",
            "http://localhost:80",
        ] {
            let mut options = options(StartRole::Join);
            options.origin = origin.into();
            refused(&options);
        }
        let mut options = options(StartRole::Host);
        options.origin = "x".repeat(4097);
        refused(&options);
    }

    #[test]
    fn bad_ca_paths_are_validated_after_complete_destination_and_origin_metadata() {
        for path in [String::new(), "ca\0.pem".into(), "x".repeat(4097)] {
            let mut options = options(StartRole::Host);
            options.ca = PathBuf::from(path);
            refused(&options);
        }
    }

    #[test]
    fn canonical_destinations_origins_and_both_roles_read_one_original_ca_and_move_its_bytes() {
        let urls = [
            "https://127.0.0.1:4433/rooms/a",
            "https://[::1]:4433/rooms/a",
            "https://relay.example/rooms/a",
            "https://relay.example:65535/rooms/Abc_12-z",
        ];
        let origins = [
            "https://ui.example",
            "https://ui.example:8443",
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://127.12.34.56:8080",
            "http://[::1]:8080",
            "https://ui.example:0",
        ];
        for role in [StartRole::Host, StartRole::Join] {
            for url in urls {
                for origin in origins {
                    let mut options = options(role);
                    options.url = url.into();
                    options.origin = origin.into();
                    let bytes = vec![17; 31];
                    let pointer = bytes.as_ptr();
                    let mut reader = reader(Some(Ok(bytes)));
                    match prepare_webtransport(&mut reader, &options) {
                        Ok(prepared) => {
                            assert!(std::ptr::eq(prepared.options, &options));
                            assert_eq!(prepared.ca.as_ptr(), pointer);
                            assert_eq!(prepared.ca, vec![17; 31]);
                        }
                        _ => {
                            panic!("canonical metadata for either client role must prepare one CA")
                        }
                    }
                    let key = options.ca.as_os_str().as_encoded_bytes();
                    assert_eq!(reader.calls, [(key.as_ptr() as usize, key.to_vec())]);
                }
            }
            let mut options = options(role);
            options.url = format!("https://relay.example/rooms/{}", "R".repeat(1024));
            options.ca = PathBuf::from("c".repeat(4096));
            let mut reader = reader(Some(Ok(vec![1])));
            assert!(prepare_webtransport(&mut reader, &options).is_ok());
            assert_eq!(reader.calls.len(), 1);
        }
    }

    #[test]
    fn opaque_read_refusal_retains_original_identity_without_retry_or_tls_processing() {
        for role in [StartRole::Host, StartRole::Join] {
            let options = options(role);
            let identity = Arc::new(53);
            let mut reader = reader(Some(Err(Opaque(identity.clone()))));
            match prepare_webtransport(&mut reader, &options) {
                Err(CredentialLoadError::Read(Opaque(actual))) => {
                    assert!(Arc::ptr_eq(&actual, &identity))
                }
                _ => panic!("reader failure must retain the original opaque associated error"),
            }
            assert_eq!(reader.calls.len(), 1);
            assert!(reader.reply.is_none());
        }
    }

    #[test]
    fn response_size_bounds_are_inclusive_and_never_cause_a_second_ca_read() {
        for role in [StartRole::Host, StartRole::Join] {
            for length in [0, 1, 1_048_576, 1_048_577] {
                let options = options(role);
                let bytes = vec![19; length];
                let pointer = bytes.as_ptr();
                let mut reader = reader(Some(Ok(bytes)));
                match prepare_webtransport(&mut reader, &options) {
                    Ok(prepared) => {
                        assert!(length == 1 || length == 1_048_576);
                        assert_eq!(prepared.ca.len(), length);
                        assert_eq!(prepared.ca.as_ptr(), pointer);
                    }
                    Err(CredentialLoadError::InvalidBytes) => {
                        assert!(length == 0 || length == 1_048_577)
                    }
                    _ => {
                        panic!("valid metadata must expose only the documented byte-bound failure")
                    }
                }
                assert_eq!(reader.calls.len(), 1);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_ca_keys_retain_original_native_spelling_without_opening_files() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let mut options = options(StartRole::Join);
        options.ca = PathBuf::from(OsString::from_vec(vec![b'c', 0xff, b'a']));
        let mut reader = reader(Some(Ok(vec![1])));
        assert!(prepare_webtransport(&mut reader, &options).is_ok());
        let key = options.ca.as_os_str().as_encoded_bytes();
        assert_eq!(
            reader.calls,
            [(key.as_ptr() as usize, vec![b'c', 0xff, b'a'])]
        );
    }
}

#[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
#[test]
fn unsupported_builds_refuse_before_even_invalid_metadata_can_acquire_ca_bytes() {
    for malformed in [false, true] {
        for role in [StartRole::Host, StartRole::Join] {
            let mut options = options(role);
            if malformed {
                options.url.clear();
                options.origin.clear();
                options.ca = PathBuf::new();
            }
            assert_eq!(
                options.validate().unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
            let mut reader = reader(None);
            match prepare_webtransport(&mut reader, &options) {
                Err(CredentialLoadError::Validation(error)) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported)
                }
                _ => panic!("unsupported builds must refuse before any reader effect"),
            }
            assert!(reader.calls.is_empty());
        }
    }
}
