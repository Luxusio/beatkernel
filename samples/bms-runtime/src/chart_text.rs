//! Bounded chart text decoding outside native and real-time owners.
use encoding_rs::{DecoderResult, SHIFT_JIS};
use std::{
    borrow::Cow,
    io::{self, Read},
};

/// Auto prefers valid UTF-8, then strict WHATWG Shift_JIS without replacement.
/// Explicit modes resolve ambiguous BOM-free input; other encodings are rejected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartTextEncoding {
    #[default]
    Auto,
    Utf8,
    ShiftJis,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8], limit: usize) -> io::Result<()> {
    let extent = output
        .len()
        .checked_add(bytes.len())
        .filter(|&extent| extent <= limit)
        .ok_or_else(|| invalid("chart text exceeds byte limit"))?;
    if extent > output.capacity() {
        // Amortize growth while capping each requested allocation, independently
        // of whether the decoder expands or contracts the original byte stream.
        let capacity = output.capacity().saturating_mul(2).max(extent).min(limit);
        output
            .try_reserve_exact(capacity - output.len())
            .map_err(|_| io::Error::other("chart text allocation failed"))?;
    }
    output.extend_from_slice(bytes);
    Ok(())
}

/// Applies independent raw and decoded UTF-8 byte limits. Only an initial UTF-8
/// BOM is stripped; it forces strict UTF-8 even in Auto. UTF-16/32 BOMs reject.
/// Valid UTF-8 is borrowed. Shift_JIS is streamed into bounded scratch/output.
pub fn decode_chart_text<'a>(
    bytes: &'a [u8],
    mode: ChartTextEncoding,
    max_bytes: usize,
) -> io::Result<Cow<'a, str>> {
    if bytes.len() > max_bytes {
        return Err(invalid("raw chart text exceeds byte limit"));
    }
    if bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || bytes.starts_with(&[0x00, 0x00, 0xfe, 0xff])
    {
        return Err(invalid("UTF-16 and UTF-32 chart text are unsupported"));
    }
    let utf8_bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    if utf8_bom && mode == ChartTextEncoding::ShiftJis {
        return Err(invalid("UTF-8 BOM conflicts with explicit Shift_JIS"));
    }
    let content = if utf8_bom { &bytes[3..] } else { bytes };
    if utf8_bom || mode != ChartTextEncoding::ShiftJis {
        match std::str::from_utf8(content) {
            Ok(text) => return Ok(Cow::Borrowed(text)),
            Err(_) if utf8_bom || mode == ChartTextEncoding::Utf8 => {
                return Err(invalid("chart text is not valid UTF-8"));
            }
            Err(_) => {}
        }
    }
    let mut decoder = SHIFT_JIS.new_decoder_without_bom_handling();
    let mut scratch = [0u8; 4096];
    let mut output = Vec::new();
    let mut offset = 0;
    loop {
        let (result, read, written) =
            decoder.decode_to_utf8_without_replacement(&content[offset..], &mut scratch, true);
        offset += read;
        if matches!(result, DecoderResult::Malformed(_, _)) {
            return Err(invalid("chart text is not valid Shift_JIS"));
        }
        append_bounded(&mut output, &scratch[..written], max_bytes)?;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(_, _) => unreachable!("malformed result rejected"),
        }
    }
    String::from_utf8(output)
        .map(Cow::Owned)
        .map_err(|_| invalid("chart decoder produced invalid UTF-8"))
}

/// Reads at most max_bytes+1 to detect oversized/growing input, retries
/// interruptions, and propagates reader failures. No filesystem is opened here.
pub fn read_chart_text(reader: &mut impl Read, max_bytes: usize) -> io::Result<String> {
    read_chart_text_with_budget(reader, max_bytes, max_bytes).1
}

/// Returns actual raw consumption even when reading, allocation or decoding fails.
/// The raw budget is independent of the decoded UTF-8 cap. One counted detection
/// byte may exceed the lesser raw limit; it is never appended or decoded.
pub fn read_chart_text_with_budget(
    reader: &mut impl Read,
    max_bytes: usize,
    raw_remaining: usize,
) -> (usize, io::Result<String>) {
    let mut consumed = 0;
    let result = (|| {
        max_bytes.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "chart reader byte limit overflow",
            )
        })?;
        let raw_limit = max_bytes.min(raw_remaining);
        let read_limit = raw_limit + 1; // The checked max_bytes bounds raw_limit.
        let mut raw = Vec::new();
        let mut scratch = [0u8; 4096];
        loop {
            let request = (read_limit - consumed).min(scratch.len());
            let read = match reader.read(&mut scratch[..request]) {
                Ok(0) => break,
                Ok(read) if read <= request => read,
                Ok(_) => return Err(invalid("chart reader returned an invalid byte count")),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            consumed += read;
            append_bounded(&mut raw, &scratch[..read], raw_limit)?;
        }
        match decode_chart_text(&raw, ChartTextEncoding::Auto, max_bytes)? {
            Cow::Owned(text) => Ok(text),
            Cow::Borrowed(text) => {
                let mut owned = String::new();
                owned
                    .try_reserve_exact(text.len())
                    .map_err(|_| io::Error::other("chart text allocation failed"))?;
                owned.push_str(text);
                Ok(owned)
            }
        }
    })();
    (consumed, result)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn literal_shift_jis_metadata_paths_halfwidth_and_cp932_extensions() {
        let bytes = b"#TITLE \x93\xfa\x96\x7b\x8c\xea\n#WAV01 sound\\\x93\xfa\x96\x7b.wav\n#BPM 60\n#00011:01\n";
        let text = decode_chart_text(bytes, ChartTextEncoding::Auto, 4096).unwrap();
        assert!(matches!(&text, Cow::Owned(_)));
        assert_eq!(
            text,
            "#TITLE 日本語\n#WAV01 sound\\日本.wav\n#BPM 60\n#00011:01\n"
        );
        let chart = beatkernel_bms::parse(&text, beatkernel_bms::ParseOptions::default()).unwrap();
        assert_eq!(chart.metadata["TITLE"], "日本語");
        assert_eq!(chart.samples[&1], "sound\\日本.wav");
        assert_eq!(
            decode_chart_text(
                b"\xb6\xc0\xb6\xc5\x87\x40\\",
                ChartTextEncoding::ShiftJis,
                32
            )
            .unwrap(),
            "ｶﾀｶﾅ①\\"
        );
    }
    #[test]
    fn utf8_borrowing_and_explicit_ambiguous_modes() {
        let bytes = "日本語".as_bytes();
        assert!(matches!(
            decode_chart_text(bytes, ChartTextEncoding::Auto, bytes.len()).unwrap(),
            Cow::Borrowed(_)
        ));
        let ambiguous = b"\xc2\xa5";
        assert_eq!(
            decode_chart_text(ambiguous, ChartTextEncoding::Auto, 8).unwrap(),
            "¥"
        );
        assert_eq!(
            decode_chart_text(ambiguous, ChartTextEncoding::Utf8, 8).unwrap(),
            "¥"
        );
        assert_eq!(
            decode_chart_text(ambiguous, ChartTextEncoding::ShiftJis, 8).unwrap(),
            "\u{ff82}\u{ff65}"
        );
        assert_eq!(
            decode_chart_text(b"", ChartTextEncoding::Auto, 0).unwrap(),
            ""
        );
        assert_eq!(
            decode_chart_text(b"ASCII\\path", ChartTextEncoding::ShiftJis, 10).unwrap(),
            "ASCII\\path"
        );
    }
    #[test]
    fn initial_utf8_bom_forces_utf8_and_other_boms_reject() {
        let bytes = b"\xef\xbb\xbfA\xef\xbb\xbf";
        assert_eq!(
            decode_chart_text(bytes, ChartTextEncoding::Auto, bytes.len()).unwrap(),
            "A\u{feff}"
        );
        assert!(matches!(
            decode_chart_text(bytes, ChartTextEncoding::Utf8, bytes.len()).unwrap(),
            Cow::Borrowed(_)
        ));
        assert!(decode_chart_text(bytes, ChartTextEncoding::ShiftJis, bytes.len()).is_err());
        assert!(decode_chart_text(b"\xef\xbb\xbf\x93\xfa", ChartTextEncoding::Auto, 10).is_err());
        for bytes in [
            &b"\xff\xfeA\0"[..],
            &b"\xfe\xff\0A"[..],
            &b"\xff\xfe\0\0"[..],
            &b"\0\0\xfe\xff"[..],
        ] {
            for mode in [
                ChartTextEncoding::Auto,
                ChartTextEncoding::Utf8,
                ChartTextEncoding::ShiftJis,
            ] {
                assert_eq!(
                    decode_chart_text(bytes, mode, 16).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
            }
        }
    }
    #[test]
    fn strict_malformed_truncated_raw_and_decoded_expansion_caps() {
        for bytes in [&b"\x81"[..], &b"\x81\x30"[..], &b"\x81\x7f"[..]] {
            assert!(decode_chart_text(bytes, ChartTextEncoding::ShiftJis, 16).is_err());
            assert!(decode_chart_text(bytes, ChartTextEncoding::Auto, 16).is_err());
        }
        assert!(decode_chart_text(b"\x93\xfa", ChartTextEncoding::Utf8, 16).is_err());
        assert!(decode_chart_text(b"abc", ChartTextEncoding::Auto, 2).is_err());
        assert!(decode_chart_text(b"\xb6", ChartTextEncoding::Auto, 2).is_err());
        assert_eq!(
            decode_chart_text(b"\xb6", ChartTextEncoding::Auto, 3).unwrap(),
            "ｶ"
        );
        assert_eq!(
            decode_chart_text(b"\x93\xfa", ChartTextEncoding::Auto, 3).unwrap(),
            "日"
        );
        let many = [0xb6; 5000];
        let text = decode_chart_text(&many, ChartTextEncoding::ShiftJis, 15_000).unwrap();
        assert_eq!(text.len(), 15_000);
        assert_eq!(text.chars().count(), 5000);
        assert!(decode_chart_text(&many, ChartTextEncoding::ShiftJis, 14_999).is_err());
    }
    struct Chunked<'a> {
        bytes: &'a [u8],
        calls: usize,
        consumed: usize,
        fail: bool,
    }
    impl Read for Chunked<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls == 1 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.fail && self.consumed > 0 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let count = out.len().min(1).min(self.bytes.len());
            out[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            self.consumed += count;
            Ok(count)
        }
    }
    #[test]
    fn reader_handles_chunked_interruptions_errors_exact_limits_and_growth_probe() {
        let mut reader = Chunked {
            bytes: b"\x93\xfa",
            calls: 0,
            consumed: 0,
            fail: false,
        };
        assert_eq!(read_chart_text(&mut reader, 3).unwrap(), "日");
        let mut reader = Chunked {
            bytes: b"abc",
            calls: 0,
            consumed: 0,
            fail: false,
        };
        assert_eq!(read_chart_text(&mut reader, 3).unwrap(), "abc");
        let mut reader = Chunked {
            bytes: b"abcdmore",
            calls: 0,
            consumed: 0,
            fail: false,
        };
        assert_eq!(
            read_chart_text(&mut reader, 3).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(reader.consumed, 4); // Only the single-byte growth probe.
        let mut reader = Chunked {
            bytes: b"abc",
            calls: 0,
            consumed: 0,
            fail: true,
        };
        assert_eq!(
            read_chart_text(&mut reader, 3).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let mut reader = &b""[..];
        assert_eq!(read_chart_text(&mut reader, 0).unwrap(), "");
        assert_eq!(
            read_chart_text(&mut reader, usize::MAX).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let mut reader = &b"\xb6"[..];
        assert!(read_chart_text(&mut reader, 2).is_err()); // Raw fits; UTF-8 does not.
    }
}
