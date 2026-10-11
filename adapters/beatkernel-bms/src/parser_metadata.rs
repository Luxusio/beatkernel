use beatkernel::audio::SampleId;

pub(super) fn note_metadata(lane: u8, head: u16, line: usize, tail: Option<SampleId>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(if tail.is_some() { 15 } else { 13 });
    bytes.extend([1, lane]);
    bytes.extend(head.to_le_bytes());
    bytes.extend((line as u64).to_le_bytes());
    if let Some(tail) = tail {
        bytes.push(1);
        bytes.extend((tail.0 as u16).to_le_bytes());
    } else {
        bytes.push(0);
    }
    bytes
}
