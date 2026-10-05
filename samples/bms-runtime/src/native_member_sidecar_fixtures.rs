//! Deferred path/byte publications using callbacks only, with no filesystem IO.
use super::*;
use crate::{
    result_archive::{decode_archive, encode_archive},
    result_archive::member_fixtures::{whole, invalid_later},
};
use std::{error::Error, fmt, sync::Arc};
#[derive(Debug)]
struct Marker(Arc<usize>);
impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "refused{}", self.0)
    }
}
impl Error for Marker {}
#[test]
fn whole_first_then_each_exact_original_member_destination_contains_only_its_row() {
    for count in 1..=64 {
        let archive = whole(count);
        let mut writes = Vec::new();
        publish_cohort_sidecars(
            &archive,
            Path::new("records/song.take.bkr"),
            |path, bytes| {
                writes.push((path.to_path_buf(), bytes.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(writes.len(), count as usize + 1);
        assert_eq!(writes[0].0, Path::new("records/song.take.bkr.bkresult"));
        assert_eq!(writes[0].1, encode_archive(&archive).unwrap());
        assert_eq!(
            writes[1..]
                .iter()
                .map(|(_, bytes)| bytes.len())
                .sum::<usize>(),
            writes[0].1.len() + 16 * (count as usize - 1)
        );
        for (index, entry) in archive.entries().iter().enumerate() {
            assert_eq!(
                writes[index + 1].0,
                Path::new(&format!(
                    "records/song.take.p{}.bkr.bkresult",
                    entry.player.0
                ))
            );
            let member = decode_archive(&writes[index + 1].1).unwrap();
            assert_eq!(member.entries(), std::slice::from_ref(entry));
            assert_eq!(
                crate::record_association::associate(&member, &entry.header, None)
                    .unwrap()
                    .player,
                entry.player
            );
        }
    }
}
#[test]
fn all_prepared_destinations_are_attempted_after_multiple_failures_returning_original_first_box() {
    let archive = whole(3);
    let tokens: [Arc<usize>; 4] = std::array::from_fn(Arc::new);
    for mask in 1..16 {
        let mut calls = Vec::new();
        let mut original: Option<*const dyn Error> = None;
        let result = publish_cohort_sidecars(&archive, Path::new("song.bkr"), |path, bytes| {
            let index = calls.len();
            calls.push(path.to_path_buf());
            assert!(decode_archive(bytes).is_ok());
            if mask & (1 << index) != 0 {
                let error: Box<dyn Error> = Box::new(Marker(tokens[index].clone()));
                if original.is_none() {
                    original = Some(error.as_ref() as *const dyn Error);
                }
                Err(error)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(calls.len(), 4);
        let first = (0..4).find(|index| mask & (1 << index) != 0).unwrap();
        assert!(Arc::ptr_eq(
            &result.downcast_ref::<Marker>().unwrap().0,
            &tokens[first]
        ));
        assert!(std::ptr::eq(
            result.as_ref() as *const dyn Error,
            original.unwrap()
        ));
    }
}
#[test]
fn invalid_whole_table_and_bad_base_path_have_no_publication_effects() {
    let mut calls = 0;
    assert!(
        publish_cohort_sidecars(&invalid_later(), Path::new("song.bkr"), |_, _| {
            calls += 1;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(calls, 0);
    for base in [Path::new(""), Path::new("/")] {
        assert!(
            publish_cohort_sidecars(&whole(2), base, |_, _| {
                calls += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(calls, 0);
    }
}
#[cfg(unix)]
#[test]
fn native_non_utf8_parent_and_filename_bytes_survive_whole_and_member_suffixes() {
    use std::os::unix::ffi::{OsStringExt, OsStrExt};
    let base = PathBuf::from(std::ffi::OsString::from_vec(
        b"parent\xff/song\xfe.take.bkr".to_vec(),
    ));
    let mut paths = Vec::new();
    publish_cohort_sidecars(&whole(1), &base, |path, _| {
        paths.push(path.as_os_str().as_bytes().to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(paths[0], b"parent\xff/song\xfe.take.bkr.bkresult");
    assert_eq!(
        paths[1],
        b"parent\xff/song\xfe.take.p4294967295.bkr.bkresult"
    );
}
