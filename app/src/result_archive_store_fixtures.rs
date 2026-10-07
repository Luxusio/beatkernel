//! Deferred scripted storage effects; no files, native IO or real clock.
use crate::{
    result_archive::{ResultArchive, MAX_ARCHIVE_BYTES},
    result_archive_store::*,
    result_archive::fixtures::{archive, golden, invalid_archive},
};
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refusal {
    operation: &'static str,
    retained_prefix: usize,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} after {} bytes", self.operation, self.retained_prefix)
    }
}
impl std::error::Error for Refusal {}
#[derive(Default)]
struct Storage {
    creates: Vec<(String, Vec<u8>)>,
    reads: Vec<(String, usize)>,
    bytes: Vec<u8>,
    refusal: Option<Refusal>,
    read_refusal: Option<Refusal>,
    partial: Option<usize>,
}
impl ResultArchiveStoragePort for Storage {
    type Error = Refusal;
    fn create_new(&mut self, key: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        self.creates.push((key.into(), bytes.to_vec()));
        if let Some(count) = self.partial {
            self.bytes = bytes[..count].to_vec();
            return Err(Refusal {
                operation: "partial write",
                retained_prefix: count,
            });
        }
        if let Some(error) = &self.refusal {
            return Err(error.clone());
        }
        self.bytes = bytes.to_vec();
        Ok(())
    }
    fn read_bounded(&mut self, key: &str, max_bytes: usize) -> Result<Vec<u8>, Self::Error> {
        self.reads.push((key.into(), max_bytes));
        if let Some(error) = &self.read_refusal {
            return Err(error.clone());
        }
        // An adversarial adapter may return too much: the policy must independently reject it.
        Ok(self.bytes.clone())
    }
}
#[test]
fn complete_validated_encoding_precedes_one_exclusive_create_and_exact_bounded_load() {
    let mut storage = Storage::default();
    let archive = archive();
    let expected = golden();
    assert_eq!(
        save_archive(&mut storage, "session.bkr-result", &archive).unwrap(),
        expected.len()
    );
    assert_eq!(
        storage.creates,
        vec![("session.bkr-result".into(), expected.clone())]
    );
    assert!(storage.reads.is_empty());
    let historical: ResultArchive = load_archive(&mut storage, "session.bkr-result").unwrap();
    assert_eq!(historical, archive);
    assert_eq!(
        storage.reads,
        vec![("session.bkr-result".into(), MAX_ARCHIVE_BYTES)]
    );
}
#[test]
fn invalid_table_has_zero_effects_and_exclusive_refusal_preserves_exact_cause() {
    let mut storage = Storage::default();
    assert!(matches!(
        save_archive(&mut storage, "bad", &invalid_archive()),
        Err(ArchiveStoreError::Archive(_))
    ));
    assert!(storage.creates.is_empty());
    assert!(storage.bytes.is_empty());
    let cause = Refusal {
        operation: "already exists",
        retained_prefix: 0,
    };
    storage.refusal = Some(cause.clone());
    storage.bytes = vec![9, 8, 7];
    match save_archive(&mut storage, "existing", &archive()) {
        Err(ArchiveStoreError::Storage(error)) => assert_eq!(error, cause),
        other => panic!("wrong result: {other:?}"),
    }
    assert_eq!(storage.creates.len(), 1);
    assert_eq!(storage.creates[0].1, golden());
    assert_eq!(storage.bytes, [9, 8, 7]);
}
#[test]
fn partial_write_retains_original_error_and_never_reports_saved_history() {
    let mut storage = Storage {
        partial: Some(17),
        ..Default::default()
    };
    match save_archive(&mut storage, "partial", &archive()) {
        Err(ArchiveStoreError::Storage(error)) => assert_eq!(
            error,
            Refusal {
                operation: "partial write",
                retained_prefix: 17
            }
        ),
        other => panic!("wrong result: {other:?}"),
    }
    assert_eq!(storage.creates.len(), 1);
    assert_eq!(storage.bytes, golden()[..17]);
    assert!(matches!(
        load_archive(&mut storage, "partial"),
        Err(ArchiveStoreError::Archive(_))
    ));
    assert_eq!(storage.creates.len(), 1);
}
#[test]
fn read_refusal_oversize_truncation_and_bad_last_row_never_return_partial_archive() {
    let cause = Refusal {
        operation: "read refused",
        retained_prefix: 0,
    };
    let mut storage = Storage {
        read_refusal: Some(cause.clone()),
        ..Default::default()
    };
    match load_archive(&mut storage, "unreadable") {
        Err(ArchiveStoreError::Storage(error)) => assert_eq!(error, cause),
        other => panic!("wrong result: {other:?}"),
    }
    storage.read_refusal = None;
    let mut bad_later = golden();
    let second = bad_later[16..].to_vec();
    bad_later[12..16].copy_from_slice(&2u32.to_le_bytes());
    bad_later.extend_from_slice(&second);
    for bytes in [
        vec![],
        golden()[..16].to_vec(),
        bad_later,
        vec![0; MAX_ARCHIVE_BYTES + 1],
    ] {
        storage.bytes = bytes;
        assert!(matches!(
            load_archive(&mut storage, "bad"),
            Err(ArchiveStoreError::Archive(_))
        ));
    }
    assert!(storage.creates.is_empty());
    assert!(
        storage
            .reads
            .iter()
            .all(|(_, limit)| *limit == MAX_ARCHIVE_BYTES)
    );
}
