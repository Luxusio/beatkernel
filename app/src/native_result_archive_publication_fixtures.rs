//! Actual native publication acceptance; no power-loss or foreign-platform claim.
use super::*;
use crate::{
    result_archive::{decode_archive, encode_archive, member_fixtures::whole},
    result_archive_store::{load_archive, save_archive},
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Barrier,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-archive-publication-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn names(&self) -> Vec<std::ffi::OsString> {
        let mut names: Vec<_> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn public_store_saves_exact_canonical_archive_and_loads_same_value() {
    let directory = Directory::new();
    let mut store = NativeResultArchiveStore::new(directory.0.clone());
    let archive = whole(2);
    let bytes = encode_archive(&archive).unwrap();
    assert_eq!(
        save_archive(&mut store, "song.bkresult", &archive).unwrap(),
        bytes.len()
    );
    assert_eq!(
        std::fs::read(directory.0.join("song.bkresult")).unwrap(),
        bytes
    );
    assert_eq!(
        store.read_bounded("song.bkresult", bytes.len()).unwrap(),
        bytes
    );
    assert_eq!(load_archive(&mut store, "song.bkresult").unwrap(), archive);
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("song.bkresult")]
    );
}

#[test]
fn store_existing_file_refusal_preserves_foreign_bytes_and_leaves_no_stage() {
    let directory = Directory::new();
    let path = directory.0.join("existing.bkresult");
    std::fs::write(&path, b"foreign incomplete archive").unwrap();
    let mut store = NativeResultArchiveStore::new(directory.0.clone());
    let error = store
        .create_new("existing.bkresult", &encode_archive(&whole(1)).unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(path).unwrap(), b"foreign incomplete archive");
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("existing.bkresult")]
    );
}

#[cfg(unix)]
#[test]
fn store_dangling_symlink_refusal_preserves_link_and_missing_target() {
    let directory = Directory::new();
    let path = directory.0.join("existing.bkresult");
    let target = directory.0.join("never-created");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let mut store = NativeResultArchiveStore::new(directory.0.clone());
    let error = store
        .create_new("existing.bkresult", &encode_archive(&whole(1)).unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_link(path).unwrap(), target);
    assert!(!target.exists());
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("existing.bkresult")]
    );
}

#[test]
fn competing_public_stores_have_one_complete_decodable_winner() {
    let directory = Directory::new();
    let barrier = Arc::new(Barrier::new(3));
    let candidates = [
        encode_archive(&whole(1)).unwrap(),
        encode_archive(&whole(2)).unwrap(),
    ];
    let handles: Vec<_> = candidates
        .iter()
        .map(|bytes| {
            let bytes = bytes.clone();
            let root = directory.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = NativeResultArchiveStore::new(root);
                barrier.wait();
                store.create_new("winner.bkresult", &bytes)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| matches!(result, Err(error) if error.kind() == io::ErrorKind::AlreadyExists)).count(), 1);
    let winner = results.iter().position(Result::is_ok).unwrap();
    let bytes = std::fs::read(directory.0.join("winner.bkresult")).unwrap();
    assert_eq!(bytes, candidates[winner]);
    assert_eq!(
        encode_archive(&decode_archive(&bytes).unwrap()).unwrap(),
        bytes
    );
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("winner.bkresult")]
    );
}

#[test]
fn invalid_store_keys_refuse_without_filesystem_effects() {
    let directory = Directory::new();
    let mut store = NativeResultArchiveStore::new(directory.0.clone());
    let bytes = encode_archive(&whole(1)).unwrap();
    let overlong = "a".repeat(256);
    for key in [
        "",
        ".",
        "..",
        "../escape",
        "nested/file",
        "nested\\file",
        "CON.bkresult",
        "LPT9.txt",
        "trailing.",
        "non-ascii-한",
        overlong.as_str(),
    ] {
        assert_eq!(
            store.create_new(key, &bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "key {key:?}"
        );
        assert!(directory.names().is_empty());
    }
}

#[test]
fn solo_sidecar_is_exact_canonical_archive_and_exclusive() {
    let directory = Directory::new();
    let base = directory.0.join("solo.take.bkr");
    let archive = whole(1);
    let bytes = encode_archive(&archive).unwrap();
    save_sidecar(&archive, &base).unwrap();
    assert_eq!(read_sidecar(&base).unwrap().unwrap(), bytes);
    assert_eq!(
        decode_archive(&std::fs::read(sidecar_path(&base).unwrap()).unwrap()).unwrap(),
        archive
    );
    let error = save_sidecar(&whole(2), &base).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(read_sidecar(&base).unwrap().unwrap(), bytes);
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("solo.take.bkr.bkresult")]
    );
}

#[cfg(unix)]
#[test]
fn solo_sidecar_dangling_symlink_refusal_preserves_link() {
    let directory = Directory::new();
    let base = directory.0.join("solo.bkr");
    let sidecar = sidecar_path(&base).unwrap();
    let target = directory.0.join("missing-target");
    std::os::unix::fs::symlink(&target, &sidecar).unwrap();
    let error = save_sidecar(&whole(1), &base).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read_link(sidecar).unwrap(), target);
    assert!(!target.exists());
    assert_eq!(
        directory.names(),
        [std::ffi::OsString::from("solo.bkr.bkresult")]
    );
}

#[test]
fn native_cohort_and_each_member_sidecar_contain_exact_canonical_projection() {
    let directory = Directory::new();
    let base = directory.0.join("cohort.bkr");
    let archive = whole(3);
    save_cohort_sidecars(&archive, &base).unwrap();
    assert_eq!(
        read_sidecar(&base).unwrap().unwrap(),
        encode_archive(&archive).unwrap()
    );
    for entry in archive.entries() {
        let member_base = crate::native_cohort::replay_path(&base, entry.player).unwrap();
        let bytes = read_sidecar(&member_base).unwrap().unwrap();
        let member = archive.for_player(entry.player).unwrap();
        assert_eq!(bytes, encode_archive(&member).unwrap());
        assert_eq!(decode_archive(&bytes).unwrap(), member);
    }
    assert_eq!(directory.names().len(), archive.entries().len() + 1);
}

#[test]
fn actual_cohort_member_refusal_preserves_foreign_file_and_publishes_later_members() {
    let directory = Directory::new();
    let base = directory.0.join("cohort.bkr");
    let archive = whole(3);
    let refused_base =
        crate::native_cohort::replay_path(&base, archive.entries()[0].player).unwrap();
    let refused_path = sidecar_path(&refused_base).unwrap();
    std::fs::write(&refused_path, b"foreign original").unwrap();
    let error = save_cohort_sidecars(&archive, &base).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(refused_path).unwrap(), b"foreign original");
    assert_eq!(
        read_sidecar(&base).unwrap().unwrap(),
        encode_archive(&archive).unwrap()
    );
    for entry in &archive.entries()[1..] {
        let member_base = crate::native_cohort::replay_path(&base, entry.player).unwrap();
        assert_eq!(
            read_sidecar(&member_base).unwrap().unwrap(),
            encode_archive(&archive.for_player(entry.player).unwrap()).unwrap()
        );
    }
    assert_eq!(directory.names().len(), archive.entries().len() + 1);
}

#[test]
fn cohort_actual_io_returns_same_first_error_box_and_attempts_all_destinations() {
    let directory = Directory::new();
    let base = directory.0.join("cohort.bkr");
    let archive = whole(3);
    let whole_path = sidecar_path(&base).unwrap();
    let first_member = sidecar_path(
        &crate::native_cohort::replay_path(&base, archive.entries()[0].player).unwrap(),
    )
    .unwrap();
    std::fs::write(&whole_path, b"foreign whole").unwrap();
    std::fs::write(&first_member, b"foreign member").unwrap();
    let mut attempts = Vec::new();
    let mut original = None;
    let error = publish_cohort_sidecars(&archive, &base, |path, bytes| {
        attempts.push(path.to_owned());
        let result = write_sidecar_bytes(path, bytes);
        if let Err(error) = &result {
            if original.is_none() {
                original = Some(error.as_ref() as *const dyn std::error::Error);
            }
        }
        result
    })
    .unwrap_err();
    assert!(std::ptr::eq(
        error.as_ref() as *const dyn std::error::Error,
        original.unwrap()
    ));
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    let mut expected = vec![whole_path.clone()];
    expected.extend(archive.entries().iter().map(|entry| {
        sidecar_path(&crate::native_cohort::replay_path(&base, entry.player).unwrap()).unwrap()
    }));
    assert_eq!(attempts, expected);
    assert_eq!(std::fs::read(whole_path).unwrap(), b"foreign whole");
    assert_eq!(std::fs::read(first_member).unwrap(), b"foreign member");
    for entry in &archive.entries()[1..] {
        let member_base = crate::native_cohort::replay_path(&base, entry.player).unwrap();
        assert_eq!(
            read_sidecar(&member_base).unwrap().unwrap(),
            encode_archive(&archive.for_player(entry.player).unwrap()).unwrap()
        );
    }
    assert_eq!(directory.names().len(), archive.entries().len() + 1);
}

#[test]
fn actual_member_paths_publish_complete_files_after_earlier_refusal() {
    let directory = Directory::new();
    let archive = whole(2);
    let base = directory.0.join("whole.retry.bkr");
    let paths: Vec<_> = archive
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            (
                entry.player,
                Some(directory.0.join(format!("actual-{index}.retry.bkr"))),
            )
        })
        .collect();
    let refused = sidecar_path(paths[0].1.as_ref().unwrap()).unwrap();
    std::fs::write(&refused, b"foreign retry").unwrap();
    let error = save_cohort_sidecars_with_paths(&archive, &base, &paths).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(refused).unwrap(), b"foreign retry");
    assert_eq!(
        read_sidecar(&base).unwrap().unwrap(),
        encode_archive(&archive).unwrap()
    );
    let later = paths[1].1.as_ref().unwrap();
    let bytes = read_sidecar(later).unwrap().unwrap();
    assert_eq!(
        bytes,
        encode_archive(&archive.for_player(paths[1].0).unwrap()).unwrap()
    );
    assert_eq!(
        decode_archive(&bytes).unwrap().entries(),
        std::slice::from_ref(&archive.entries()[1])
    );
    assert_eq!(directory.names().len(), 3);
}
