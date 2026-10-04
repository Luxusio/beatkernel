//! Deferred real decoder/reader fixtures; this module never opens a filesystem.
use crate::chart_text::{read_chart_text, read_chart_text_with_budget};
use std::io::{self, Read};

struct Reader<'a> {
    bytes: &'a [u8],
    consumed: usize,
    chunk: usize,
    interruptions: usize,
    failure_at: Option<usize>,
    requests: Vec<usize>,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            bytes,
            consumed: 0,
            chunk,
            interruptions: 0,
            failure_at: None,
            requests: Vec::new(),
        }
    }
}
impl Read for Reader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.requests.push(output.len());
        if self.interruptions > 0 {
            self.interruptions -= 1;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self
            .failure_at
            .is_some_and(|offset| self.consumed >= offset)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture read denied after its original prefix",
            ));
        }
        let available = self
            .failure_at
            .map_or(usize::MAX, |offset| offset - self.consumed);
        let count = output
            .len()
            .min(self.chunk)
            .min(self.bytes.len() - self.consumed)
            .min(available);
        output[..count].copy_from_slice(&self.bytes[self.consumed..self.consumed + count]);
        self.consumed += count;
        Ok(count)
    }
}

#[test]
fn counted_reads_preserve_utf8_bom_and_shift_jis_with_independent_raw_and_decoded_limits() {
    for (bytes, decoded_limit, raw_limit, expected) in [
        (&b"abc"[..], 3, 3, "abc"),
        (&b"\xef\xbb\xbfA"[..], 4, 4, "A"),
        (&b"\xef\xbb\xbfA\xef\xbb\xbf"[..], 7, 7, "A\u{feff}"),
        (&b"\x93\xfa"[..], 3, 2, "日"),
        (&b"\xb6"[..], 3, 1, "ｶ"),
        (&b""[..], 0, 0, ""),
        (&b"abc"[..], 3, usize::MAX, "abc"),
    ] {
        for chunk in [1, 2, 4096] {
            let original = bytes.to_vec();
            let mut reader = Reader::new(bytes, chunk);
            reader.interruptions = 2;
            let (count, result) =
                read_chart_text_with_budget(&mut reader, decoded_limit, raw_limit);
            assert_eq!(result.unwrap(), expected);
            assert_eq!(count, bytes.len());
            assert_eq!(reader.consumed, count);
            assert_eq!(bytes, original);
            assert!(
                reader
                    .requests
                    .iter()
                    .all(|&request| request > 0 && request <= decoded_limit.min(raw_limit) + 1)
            );
            let mut legacy = Reader::new(bytes, chunk);
            assert_eq!(
                read_chart_text(&mut legacy, decoded_limit).unwrap(),
                expected
            );
        }
    }
    // Valid raw Shift_JIS still fails when its decoded UTF-8 expansion exceeds
    // the independent text cap. No String containing a partial prefix escapes.
    for (bytes, decoded_limit, raw_limit, expected_count) in [
        (&b"\xb6"[..], 2, 1, 1),
        (&b"\x93\xfa"[..], 2, 2, 2),
        (&b"\x81"[..], 16, 16, 1),
        (&b"\xff\xfeA\0"[..], 16, 16, 4),
        (&b"\xef\xbb\xbf\x93\xfa"[..], 16, 16, 5),
    ] {
        let mut reader = Reader::new(bytes, 1);
        let (count, result) = read_chart_text_with_budget(&mut reader, decoded_limit, raw_limit);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(count, expected_count);
        assert_eq!(reader.consumed, expected_count);
    }
}

#[test]
fn detection_bytes_and_failed_read_prefixes_are_charged_once_without_overreading_or_overflow() {
    for (decoded_limit, raw_limit, expected_count) in
        [(20, 3, 4), (3, 20, 4), (3, 3, 4), (20, 0, 1)]
    {
        for chunk in [1, 4096] {
            let mut reader = Reader::new(b"abcdefghij", chunk);
            let (count, result) =
                read_chart_text_with_budget(&mut reader, decoded_limit, raw_limit);
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(count, expected_count);
            assert_eq!(reader.consumed, expected_count);
            assert_eq!(
                &reader.bytes[reader.consumed..],
                &b"abcdefghij"[expected_count..]
            );
        }
    }
    let large = vec![b'a'; 8192];
    let mut reader = Reader::new(&large, usize::MAX);
    let (count, result) = read_chart_text_with_budget(&mut reader, 8192, 4096);
    assert!(result.is_err());
    assert_eq!(count, 4097);
    assert_eq!(reader.consumed, 4097);
    assert_eq!(
        reader.requests.last(),
        Some(&1),
        "only one detection byte is requested after the full budget"
    );

    for failure_at in [0, 3] {
        let mut reader = Reader::new(b"abcdefghij", 2);
        reader.interruptions = 2;
        reader.failure_at = Some(failure_at);
        let (count, result) = read_chart_text_with_budget(&mut reader, 20, 20);
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.to_string(),
            "fixture read denied after its original prefix"
        );
        assert_eq!(count, failure_at);
        assert_eq!(reader.consumed, failure_at);
    }
    for raw_limit in [0, 3, usize::MAX] {
        let mut reader = Reader::new(b"never consumed", 1);
        let (count, result) = read_chart_text_with_budget(&mut reader, usize::MAX, raw_limit);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(count, 0);
        assert!(reader.requests.is_empty());
        assert_eq!(reader.consumed, 0);
    }
}
