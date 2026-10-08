//! Authored sequential links: expected extents come from each fixture's trim.
use crate::{
    vorbis_decode::VorbisDecoder,
    vorbis_fixture::{continued_comment, link, packed_audio, pages, reseal_page, silence},
    AssetDecoder,
};
use beatkernel::audio::{AudioFormat, PcmLimits, PcmSample};
use std::{error::Error, path::Path};

fn decode(bytes: &[u8], cap: usize) -> Result<PcmSample, Box<dyn Error>> {
    VorbisDecoder.decode(
        Path::new("authored-chain.ogg"),
        bytes,
        PcmLimits::new(cap, cap, 4).unwrap(),
    )
}

#[test]
fn two_and_three_links_trim_independently_and_reset_each_packet_window() {
    for channels in [1u8, 2, 8, 32] {
        for trims in [[1u64, 48, 0], [0, 0, 97], [32, 1, 64]] {
            let first = link(silence(channels, trims[0]), 101, 24_000);
            let second = link(packed_audio(channels, trims[1]), 202, 24_000);
            let third = link(continued_comment(channels, trims[2]), 303, 24_000);
            for streams in [
                vec![first.clone(), second.clone()],
                vec![first.clone(), second.clone(), third],
            ] {
                let expected_frames: u64 = trims[..streams.len()].iter().sum();
                let bytes = streams.concat();
                let expected_samples = expected_frames as usize * usize::from(channels);
                let cap = (expected_samples * 4).max(1);
                let pcm = decode(&bytes, cap).unwrap();
                assert_eq!(
                    pcm.format(),
                    AudioFormat::new(24_000, u16::from(channels)).unwrap()
                );
                assert_eq!(pcm.frames(), expected_frames as usize);
                assert_eq!(pcm.samples(), vec![0.0f32; expected_samples]);
                if cap > 1 {
                    assert!(
                        decode(&bytes, cap - 1).is_err(),
                        "whole-chain cap, not per-link cap"
                    );
                }
            }
        }
    }
}

#[test]
fn a_chain_of_empty_links_and_old_single_stream_remain_valid() {
    let empty = [
        link(silence(2, 0), 11, 24_000),
        link(packed_audio(2, 0), 22, 24_000),
    ]
    .concat();
    assert_eq!(decode(&empty, 1).unwrap().frames(), 0);
    for frames in [0u64, 1, 48, 97] {
        let single = silence(2, frames);
        let pcm = decode(&single, (frames as usize * 8).max(1)).unwrap();
        assert_eq!(pcm.frames(), frames as usize);
        assert_eq!(pcm.samples(), vec![0.0f32; frames as usize * 2]);
    }
}

#[test]
fn later_crc_headers_sequences_continuations_and_eos_refuse_the_whole_asset() {
    let first = link(silence(1, 48), 11, 24_000);
    let second = link(continued_comment(1, 97), 22, 24_000);
    let offsets = pages(&second);
    let mut damaged = Vec::new();

    let mut crc = second.clone();
    crc[22] ^= 1;
    damaged.push(crc);

    for (offset, index, value) in [
        (0, 5, 0),                         // Missing second BOS.
        (0, 28, 0),                        // Unsupported identification packet.
        (0, 18, 1),                        // Second link must start sequence zero.
        (offsets[2].0, 5, 0),              // Continued comment loses continuation flag.
        (offsets[3].0, 18, 99),            // Interior nonconsecutive sequence.
        (offsets.last().unwrap().0, 5, 0), // Missing final EOS.
    ] {
        let mut changed = second.clone();
        changed[offset + index] = value;
        reseal_page(&mut changed, offset);
        damaged.push(changed);
    }
    let mut header_eos = second[..offsets[3].0 + offsets[3].1].to_vec();
    header_eos[offsets[3].0 + 5] = 4;
    reseal_page(&mut header_eos, offsets[3].0);
    damaged.push(header_eos);
    damaged.push(second[..second.len() - 1].to_vec());
    let mut trailing = second.clone();
    trailing.push(0);
    damaged.push(trailing);

    for (case, later) in damaged.into_iter().enumerate() {
        let whole = [first.clone(), later].concat();
        assert!(
            decode(&whole, 4096).is_err(),
            "damaged later link case {case}"
        );
    }
}

#[test]
fn overlapping_or_incomplete_link_boundary_and_multiplexed_pages_refuse() {
    let first = link(silence(1, 48), 11, 24_000);
    let second = link(silence(1, 97), 22, 24_000);
    let first_pages = pages(&first);
    let second_pages = pages(&second);
    let mut missing_eos = first.clone();
    let last = first_pages.last().unwrap().0;
    missing_eos[last + 5] = 0;
    reseal_page(&mut missing_eos, last);
    assert!(decode(&[missing_eos, second.clone()].concat(), 4096).is_err());

    let mut unfinished = link(continued_comment(1, 48), 11, 24_000);
    let cut = pages(&unfinished)[2].0;
    unfinished.truncate(cut);
    assert!(decode(&[unfinished, second.clone()].concat(), 4096).is_err());

    let split = first_pages[3].0;
    let foreign_page_end = second_pages[0].1;
    let multiplexed = [
        first[..split].to_vec(),
        second[..foreign_page_end].to_vec(),
        first[split..].to_vec(),
        second[foreign_page_end..].to_vec(),
    ]
    .concat();
    assert!(decode(&multiplexed, 4096).is_err());
}

#[test]
fn repeated_serial_refuses_even_with_complete_independently_valid_links() {
    let first = link(silence(1, 48), 11, 24_000);
    let second = link(packed_audio(1, 97), 11, 24_000);
    assert_eq!(decode(&first, 4096).unwrap().frames(), 48);
    assert_eq!(decode(&second, 4096).unwrap().frames(), 97);
    assert!(decode(&[first, second].concat(), 4096).is_err());
}

#[test]
fn changed_channels_or_rate_and_checked_aggregate_granule_overflow_refuse() {
    let first = link(silence(1, 48), 11, 24_000);
    for second in [
        link(silence(2, 48), 22, 24_000),
        link(silence(1, 48), 22, 48_000),
    ] {
        assert!(decode(&[first.clone(), second].concat(), 4096).is_err());
    }
    let mut enormous = first;
    let last = pages(&enormous).last().unwrap().0;
    enormous[last + 6..last + 14].copy_from_slice(&(u64::MAX - 1).to_le_bytes());
    reseal_page(&mut enormous, last);
    let second = link(silence(1, 97), 22, 24_000);
    assert!(decode(&[enormous, second].concat(), 4096).is_err());
}
