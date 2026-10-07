use super::*;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
};

// No Clone, Debug, Display or std::error::Error implementation.
struct ReadToken(Arc<u8>);
struct Reader {
    replies: VecDeque<Result<Vec<u8>, ReadToken>>,
    calls: Vec<(usize, Vec<u8>)>,
}
impl CredentialReadPort for Reader {
    type Error = ReadToken;
    fn read(&mut self, path: &Path) -> Result<Vec<u8>, ReadToken> {
        let bytes = path.as_os_str().as_encoded_bytes();
        self.calls.push((bytes.as_ptr() as usize, bytes.to_vec()));
        self.replies
            .pop_front()
            .expect("unexpected credential acquisition")
    }
}
fn reader(replies: impl IntoIterator<Item = Result<Vec<u8>, ReadToken>>) -> Reader {
    Reader {
        replies: replies.into_iter().collect(),
        calls: vec![],
    }
}
fn host() -> QuicCredentials {
    QuicCredentials {
        cert: Some(PathBuf::from("original-cert.pem")),
        key: Some(PathBuf::from("original-key.pem")),
        ca: None,
        server_name: None,
    }
}
fn join() -> QuicCredentials {
    QuicCredentials {
        cert: None,
        key: None,
        ca: Some(PathBuf::from("original-ca.pem")),
        server_name: Some("rhythm.example".into()),
    }
}
fn key_identity(path: &Path) -> (usize, Vec<u8>) {
    let bytes = path.as_os_str().as_encoded_bytes();
    (bytes.as_ptr() as usize, bytes.to_vec())
}
fn validation_refuses(credentials: &QuicCredentials, hosting: bool) {
    let mut reader = reader([]);
    match load_credentials(&mut reader, credentials, hosting) {
        Err(CredentialLoadError::Validation(error)) => {
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput)
        }
        _ => panic!("invalid complete metadata must refuse before credential reads"),
    }
    assert!(reader.calls.is_empty());
}

#[test]
fn every_incomplete_or_forbidden_role_combination_refuses_before_any_read() {
    for hosting in [false, true] {
        for mask in 0..16 {
            let credentials = QuicCredentials {
                cert: (mask & 1 != 0).then(|| PathBuf::from("cert")),
                key: (mask & 2 != 0).then(|| PathBuf::from("key")),
                ca: (mask & 4 != 0).then(|| PathBuf::from("ca")),
                server_name: (mask & 8 != 0).then(|| "rhythm.example".into()),
            };
            if mask != if hosting { 3 } else { 12 } {
                validation_refuses(&credentials, hosting);
            }
        }
    }
}

#[test]
fn malformed_paths_and_names_are_preflighted_even_when_a_prior_path_is_valid() {
    for spelling in [String::new(), "a\0b".into(), "x".repeat(4097)] {
        let mut credentials = host();
        credentials.cert = Some(PathBuf::from(&spelling));
        validation_refuses(&credentials, true);
        let mut credentials = host();
        credentials.key = Some(PathBuf::from(&spelling));
        validation_refuses(&credentials, true);
        let mut credentials = join();
        credentials.ca = Some(PathBuf::from(&spelling));
        validation_refuses(&credentials, false);
    }
    for name in [
        "",
        ".",
        "-bad.example",
        "bad-.example",
        "a..example",
        "bad_name.example",
        "host.123",
        "例.example",
        "bad\0.example",
        "bad\n.example",
    ] {
        let mut credentials = join();
        credentials.server_name = Some(name.into());
        validation_refuses(&credentials, false);
    }
    for name in [format!("{}.example", "x".repeat(64)), "x".repeat(254)] {
        let mut credentials = join();
        credentials.server_name = Some(name);
        validation_refuses(&credentials, false);
    }
}

#[test]
fn successful_roles_borrow_original_keys_and_name_and_move_owned_byte_buffers() {
    let credentials = host();
    let cert = vec![11; 19];
    let cert_pointer = cert.as_ptr();
    let key = vec![23; 29];
    let key_pointer = key.as_ptr();
    let mut host_reader = reader([Ok(cert), Ok(key)]);
    match load_credentials(&mut host_reader, &credentials, true) {
        Ok(LoadedCredentials::Host { cert, key }) => {
            assert_eq!(cert.as_ptr(), cert_pointer);
            assert_eq!(key.as_ptr(), key_pointer);
            assert_eq!(cert, vec![11; 19]);
            assert_eq!(key, vec![23; 29]);
        }
        _ => panic!("complete host metadata must return the original owned buffers"),
    }
    assert_eq!(
        host_reader.calls,
        [
            key_identity(credentials.cert.as_deref().unwrap()),
            key_identity(credentials.key.as_deref().unwrap())
        ]
    );

    let max_dns = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    for name in [
        "rhythm.example".to_owned(),
        "rhythm.example.".into(),
        "127.0.0.1".into(),
        "::1".into(),
        max_dns,
    ] {
        let mut credentials = join();
        credentials.server_name = Some(name);
        credentials.ca = Some(PathBuf::from("p".repeat(4096)));
        let ca = vec![31; 37];
        let pointer = ca.as_ptr();
        let mut reader = reader([Ok(ca)]);
        match load_credentials(&mut reader, &credentials, false) {
            Ok(LoadedCredentials::Join { ca, server_name }) => {
                assert_eq!(ca.as_ptr(), pointer);
                assert_eq!(ca, vec![31; 37]);
                let original = credentials.server_name.as_deref().unwrap();
                assert_eq!(server_name, original);
                assert_eq!(server_name.as_ptr(), original.as_ptr());
            }
            _ => panic!("join must acquire only its CA and borrow the original valid name"),
        }
        assert_eq!(
            reader.calls,
            [key_identity(credentials.ca.as_deref().unwrap())]
        );
    }
}

#[test]
fn each_read_refusal_retains_exact_opaque_error_and_stops_subsequent_acquisition() {
    for (hosting, failed) in [(true, 0), (true, 1), (false, 0)] {
        let credentials = if hosting { host() } else { join() };
        let token = Arc::new(71);
        let mut replies = Vec::new();
        for _ in 0..failed {
            replies.push(Ok(vec![1]));
        }
        replies.push(Err(ReadToken(token.clone())));
        replies.push(Ok(vec![99]));
        let mut reader = reader(replies);
        match load_credentials(&mut reader, &credentials, hosting) {
            Err(CredentialLoadError::Read(ReadToken(actual))) => {
                assert!(Arc::ptr_eq(&actual, &token))
            }
            _ => panic!("reader refusal must retain the original associated token"),
        }
        assert_eq!(reader.calls.len(), failed + 1);
        assert_eq!(reader.replies.len(), 1);
    }
}

#[test]
fn byte_bounds_are_inclusive_and_each_invalid_response_stops_later_reads() {
    assert_eq!(CREDENTIAL_BYTE_LIMIT, 1_048_576);
    for length in [0, 1, 1_048_576, 1_048_577] {
        let valid = length == 1 || length == 1_048_576;
        for (hosting, index) in [(true, 0), (true, 1), (false, 0)] {
            let credentials = if hosting { host() } else { join() };
            let mut replies = Vec::new();
            if index == 1 {
                replies.push(Ok(vec![7]));
            }
            replies.push(Ok(vec![13; length]));
            if hosting && index == 0 {
                replies.push(Ok(vec![19]));
            }
            replies.push(Ok(vec![97]));
            let mut reader = reader(replies);
            let result = load_credentials(&mut reader, &credentials, hosting);
            if valid {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(CredentialLoadError::InvalidBytes)));
            }
            assert_eq!(
                reader.calls.len(),
                if valid && hosting { 2 } else { index + 1 }
            );
            assert_eq!(
                reader.replies.len(),
                if !valid && hosting && index == 0 {
                    2
                } else {
                    1
                }
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_native_path_spelling_is_borrowed_unchanged_without_filesystem_access() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut credentials = host();
    credentials.cert = Some(PathBuf::from(OsString::from_vec(vec![
        b'c', 0xff, b'.', b'p',
    ])));
    let mut reader = reader([Ok(vec![1]), Ok(vec![2])]);
    assert!(load_credentials(&mut reader, &credentials, true).is_ok());
    assert_eq!(
        reader.calls[0],
        key_identity(credentials.cert.as_deref().unwrap())
    );
    assert_eq!(reader.calls[0].1, [b'c', 0xff, b'.', b'p']);
}
