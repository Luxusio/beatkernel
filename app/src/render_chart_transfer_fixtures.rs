//! Receiver registration fixtures built from real parsed/compiled BMS and asset preparation.
use super::{PlayerChart, PlayerChartTransfer, PlayerNote};
use crate::{
    asset_source::{MemoryAssetLimits, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets, ImageAssetsTransfer, ImageUnavailable},
    native_judge::NativeJudgeConfig,
    note_progress::NoteProgress,
    texture::RgbaImage,
};
use beatkernel::{
    chart::{MAX_SOURCE_ITEMS, ObjectId},
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::{BmsChart, ImageId, parse};
use std::sync::Arc;

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}

const CHART: &str = "#TITLE 별빛\n#ARTIST 作曲家\n#BPM 60\n#BPM01 120\n#STOP01 48\n#POORBGA 1\n#LNOBJ ZZ\n#WAV01 head.wav\n#BMP00 poor.bmp\n#00011:010000ZZ\n#00016:0101\n#00021:0001\n#00008:00010000\n#00009:00010000\n#000D2:1EZZ\n#002E6:01\n#00336:01\n#00004:0100\n#00007:02\n#0010A:03\n#00106:04\n#0000B:01\n#0010C:80\n#0010D:7F\n#0010E:02\n";

fn real_chart() -> (BmsChart, PlayerChart) {
    let source = parse(CHART, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let chart = PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
    (source, chart)
}

#[test]
fn real_bms_registration_preserves_compiled_objects_mines_and_time_prefixes() {
    let (source, original) = real_chart();
    let data = original.export_visual();
    assert_eq!(data.initial_poor, Some(ImageId(0)));
    assert_eq!(data.bga, source.compile_bga().unwrap());
    assert_eq!(data.opacity, source.compile_bga_opacity().unwrap());
    let received = PlayerChart::import_visual(data).unwrap();
    assert_eq!(received.title, "별빛");
    assert_eq!(received.artist, "作曲家");
    assert_eq!(received.notes, original.notes);
    assert_eq!(received.lanes, original.lanes);
    assert_eq!(received.mines(), original.mines());
    assert_eq!(received.duration_ns, original.duration_ns);
    assert_eq!(received.poor_bga_mode, original.poor_bga_mode);
    assert!(received.notes.iter().any(|note| note.end.is_some()));
    assert!(!received.mines().is_empty());
    for note in &original.notes {
        assert_eq!(received.note_by_object(note.object), Some(note));
        assert_eq!(
            received.note_index_by_object(note.object),
            original.note_index_by_object(note.object)
        );
    }
    assert!(received.note_by_object(ObjectId(u64::MAX)).is_none());
    let mut times = vec![i64::MIN, -1, 0, 1, i64::MAX];
    for note in &original.notes {
        times.extend([
            note.start.as_nanos() - 1,
            note.start.as_nanos(),
            note.start.as_nanos() + 1,
        ]);
        if let Some(end) = note.end {
            times.extend([end.as_nanos(), end.as_nanos() + 1]);
        }
    }
    for mine in original.mines() {
        times.extend([
            mine.at.as_nanos() - 1,
            mine.at.as_nanos(),
            mine.at.as_nanos() + 1,
        ]);
    }
    for event in source.compile_bga().unwrap() {
        times.extend([
            event.at.as_nanos() - 1,
            event.at.as_nanos(),
            event.at.as_nanos() + 1,
        ]);
    }
    for event in source.compile_bga_opacity().unwrap() {
        times.extend([
            event.at.as_nanos() - 1,
            event.at.as_nanos(),
            event.at.as_nanos() + 1,
        ]);
    }
    // Reverse iteration also proves these timelines are indexed prefixes, not advancing cursors.
    for now in times.into_iter().rev().map(ts) {
        assert_eq!(received.bga_state(now), original.bga_state(now));
        assert_eq!(received.bga_opacity(now), original.bga_opacity(now));
        for (ahead, behind) in [
            (0, 0),
            (500_000_000, 100_000_000),
            (i64::MAX, i64::MAX),
            (-1, 0),
        ] {
            let mut left = Vec::new();
            let mut right = Vec::new();
            assert_eq!(
                received.visible_note_indices_checked(now, ahead, behind, &mut left),
                original.visible_note_indices_checked(now, ahead, behind, &mut right)
            );
            assert_eq!(left, right);
            assert_eq!(
                received.visible_mine_indices_checked(now, ahead, behind, &mut left),
                original.visible_mine_indices_checked(now, ahead, behind, &mut right)
            );
            assert_eq!(left, right);
        }
    }
}

#[test]
fn separately_allocated_receiver_uses_real_judge_events_and_rejects_foreign_progress() {
    let (source, original) = real_chart();
    let original = Arc::new(original);
    let receiver = Arc::new(PlayerChart::import_visual(original.export_visual()).unwrap());
    assert!(!Arc::ptr_eq(&original, &receiver));
    let mut judge = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(7),
        end: None,
    }
    .judge(&source, source.compile().unwrap().chart)
    .unwrap();
    let events = judge.advance_to(ts(original.duration_ns + 1)).unwrap();
    assert!(!events.is_empty());
    let mut producer = NoteProgress::new(original.clone()).unwrap();
    let mut consumer = NoteProgress::new(receiver.clone()).unwrap();
    producer.apply(&events);
    consumer.apply(&events);
    let mut left = Vec::new();
    let mut right = Vec::new();
    original
        .visible_note_indices_with_progress_checked(ts(0), i64::MAX, 0, Some(&producer), &mut left)
        .unwrap();
    receiver
        .visible_note_indices_with_progress_checked(ts(0), i64::MAX, 0, Some(&consumer), &mut right)
        .unwrap();
    assert_eq!(left, right);
    assert!(left.is_empty());
    assert!(
        receiver
            .visible_note_indices_with_progress_checked(
                ts(0),
                i64::MAX,
                0,
                Some(&producer),
                &mut right
            )
            .is_err()
    );
    assert!(right.is_empty());
}

fn rejects_chart(mut change: impl FnMut(&mut PlayerChartTransfer)) {
    let (_, original) = real_chart();
    let mut candidate = original.export_visual();
    change(&mut candidate);
    assert!(PlayerChart::import_visual(candidate).is_err());
    // Rejection cannot corrupt the reusable authoritative owner's indexed queries.
    assert_eq!(
        original.note_by_object(original.notes[0].object),
        Some(&original.notes[0])
    );
    assert!(PlayerChart::import_visual(original.export_visual()).is_ok());
}

#[test]
fn chart_registration_refuses_hostile_last_items_and_inconsistent_extents() {
    rejects_chart(|data| {
        let object = data.notes[0].object;
        data.notes.last_mut().unwrap().object = object;
    });
    rejects_chart(|data| {
        data.notes.last_mut().unwrap().lane_index = data.lanes.len();
    });
    rejects_chart(|data| {
        data.notes.last_mut().unwrap().start = ts(-1);
    });
    rejects_chart(|data| {
        data.notes[0].end = Some(ts(-1));
    });
    rejects_chart(|data| {
        data.notes.swap(0, 1);
    });
    rejects_chart(|data| {
        data.duration_ns = 0;
    });
    rejects_chart(|data| {
        data.lanes.push(data.lanes[0]);
    });
    rejects_chart(|data| {
        data.mines.last_mut().unwrap().lane_index = data.lanes.len();
    });
    rejects_chart(|data| {
        data.mines.swap(0, 1);
    });
    rejects_chart(|data| {
        data.bga.last_mut().unwrap().at = ts(-1);
    });
    rejects_chart(|data| {
        data.opacity.last_mut().unwrap().at = ts(-1);
    });
}

#[test]
fn chart_accepts_original_source_ceiling_and_rejects_one_extra_without_duplicate_ids() {
    let (_, original) = real_chart();
    let mut data = original.export_visual();
    data.mines.clear();
    data.bga.clear();
    data.opacity.clear();
    data.lanes = vec![0x11];
    data.duration_ns = MAX_SOURCE_ITEMS as i64;
    data.notes = (0..MAX_SOURCE_ITEMS)
        .map(|index| PlayerNote {
            object: ObjectId(index as u64 + 1),
            lane_index: 0,
            start: ts(index as i64),
            end: None,
        })
        .collect();
    let chart = PlayerChart::import_visual(data).unwrap();
    assert_eq!(chart.notes.len(), MAX_SOURCE_ITEMS);
    assert_eq!(
        chart.note_index_by_object(ObjectId(MAX_SOURCE_ITEMS as u64)),
        Some(MAX_SOURCE_ITEMS - 1)
    );
    let mut excessive = chart.export_visual();
    excessive.notes.push(PlayerNote {
        object: ObjectId(MAX_SOURCE_ITEMS as u64 + 1),
        lane_index: 0,
        start: ts(MAX_SOURCE_ITEMS as i64),
        end: None,
    });
    assert!(PlayerChart::import_visual(excessive).is_err());
}

#[test]
fn empty_real_chart_and_empty_asset_registration_keep_pristine_behavior() {
    let source = parse("#TITLE 空\n#BPM 120\n", Default::default()).unwrap();
    let original = PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
    let receiver = PlayerChart::import_visual(original.export_visual()).unwrap();
    assert!(receiver.notes.is_empty());
    assert!(receiver.lanes.is_empty());
    assert!(receiver.mines().is_empty());
    assert_eq!(receiver.duration_ns, 0);
    assert_eq!(receiver.bga_state(ts(-1)), crate::bga::BgaState::default());
    assert_eq!(
        receiver.bga_opacity(ts(i64::MAX)),
        crate::bga_opacity::BgaOpacity::default()
    );
    let images = ImageAssets::import_visual(
        ImageAssets::default().export_visual(),
        ImageAssetLimits::default(),
    )
    .unwrap();
    assert!(images.is_empty());
    assert_eq!(images.decoded_bytes(), 0);
    assert_eq!(images.unique_images(), 0);
}

fn raster() -> Vec<u8> {
    use image::ImageEncoder;
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(
            &[0, 0, 0, 255, 9, 8, 7, 128],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    encoded
}

fn prepared_images() -> ImageAssets {
    let text = "#CANVASSIZE 2 1\n#BMP00 art.png\n#BMP01 ./art.png\n#BMP02 art.png\n#BGA03 01 0 0 1 1 1 0\n#BGA04 02 0 0 1 1 1 0\n#BMP05 absent.bmp\n#BMP06 bad.bin\n#BMP08 damaged.png\n#00004:0102030405060708\n#00007:0103\n#0000A:0204\n";
    let chart = parse(text, Default::default()).unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", text.as_bytes().to_vec())
        .unwrap();
    files.insert("song/art.png", raster()).unwrap();
    files
        .insert("song/bad.bin", b"unsupported".to_vec())
        .unwrap();
    files
        .insert("song/damaged.png", b"\x89PNG\r\n\x1a\n".to_vec())
        .unwrap();
    ImageAssets::prepare_from_source(
        &files.scope("song/chart.bms").unwrap(),
        &chart,
        ImageAssetLimits::default(),
    )
    .unwrap()
}

fn copied_pixels(mut data: ImageAssetsTransfer) -> ImageAssetsTransfer {
    // Reconstruct independently owned wire bytes while preserving table aliases.
    data.resources = data
        .resources
        .iter()
        .map(|image| {
            Arc::new(
                RgbaImage::new(image.width(), image.height(), image.pixels().to_vec()).unwrap(),
            )
        })
        .collect();
    data
}

#[test]
fn genuine_asset_preparation_round_trip_preserves_roles_aliases_and_blank_reasons() {
    let original = prepared_images();
    let received = ImageAssets::import_visual(
        copied_pixels(original.export_visual()),
        ImageAssetLimits::default(),
    )
    .unwrap();
    assert_eq!(received.len(), original.len());
    assert_eq!(received.decoded_bytes(), original.decoded_bytes());
    assert_eq!(received.unique_images(), original.unique_images());
    for id in (0..=8).map(ImageId) {
        for (left, right) in [
            (received.get(id), original.get(id)),
            (received.get_layer(id), original.get_layer(id)),
        ] {
            assert_eq!(
                left.map(|image| (image.width(), image.height(), image.pixels())),
                right.map(|image| (image.width(), image.height(), image.pixels()))
            );
        }
        assert_eq!(received.unavailable(id), original.unavailable(id));
    }
    assert_eq!(
        received.get(ImageId(0)).unwrap().pixels(),
        &[0, 0, 0, 255, 9, 8, 7, 128]
    );
    assert_eq!(
        received.get_layer(ImageId(1)).unwrap().pixels(),
        &[0, 0, 0, 0, 9, 8, 7, 128]
    );
    assert!(received.get_layer(ImageId(0)).is_none());
    assert!(Arc::ptr_eq(
        received.get(ImageId(0)).unwrap(),
        received.get(ImageId(2)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        received.get(ImageId(3)).unwrap(),
        received.get(ImageId(4)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        received.get_layer(ImageId(1)).unwrap(),
        received.get_layer(ImageId(2)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        received.get_layer(ImageId(3)).unwrap(),
        received.get_layer(ImageId(4)).unwrap()
    ));
    assert!(!Arc::ptr_eq(
        received.get(ImageId(1)).unwrap(),
        original.get(ImageId(1)).unwrap()
    ));
    assert_eq!(
        received.unavailable(ImageId(5)),
        Some(&ImageUnavailable::Missing)
    );
    assert_eq!(
        received.unavailable(ImageId(6)),
        Some(&ImageUnavailable::Unsupported)
    );
    assert_eq!(
        received.unavailable(ImageId(7)),
        Some(&ImageUnavailable::Undefined)
    );
    assert!(matches!(
        received.unavailable(ImageId(8)),
        Some(ImageUnavailable::InvalidData(_))
    ));
}

#[test]
fn image_limits_count_unique_allocations_and_allow_exact_preparation_budgets() {
    let bank = prepared_images();
    let exact = ImageAssetLimits {
        max_images: bank.len(),
        max_decoded_bytes: bank.decoded_bytes(),
        ..Default::default()
    };
    assert!(ImageAssets::import_visual(bank.export_visual(), exact).is_ok());
    assert!(
        ImageAssets::import_visual(
            bank.export_visual(),
            ImageAssetLimits {
                max_images: bank.len() - 1,
                ..exact
            }
        )
        .is_err()
    );
    assert!(
        ImageAssets::import_visual(
            bank.export_visual(),
            ImageAssetLimits {
                max_decoded_bytes: bank.decoded_bytes() - 1,
                ..exact
            }
        )
        .is_err()
    );
    let mut narrow = exact;
    narrow.decode.max_width = 1;
    assert!(ImageAssets::import_visual(bank.export_visual(), narrow).is_err());
    let mut short = exact;
    short.decode.max_decoded_bytes = 7;
    assert!(ImageAssets::import_visual(bank.export_visual(), short).is_err());
    let mut zero = exact;
    zero.decode.max_height = 0;
    assert!(ImageAssets::import_visual(bank.export_visual(), zero).is_err());
}

#[test]
fn image_registration_rejects_hostile_aliases_and_never_publishes_invalid_last_item() {
    let bank = prepared_images();
    for corrupt in 0..9 {
        let mut data = bank.export_visual();
        match corrupt {
            0 => data.images.last_mut().unwrap().1 = data.resources.len(),
            1 => data.images.push(data.images[0]),
            2 => data
                .unavailable
                .push((data.images[0].0, ImageUnavailable::Undefined)),
            3 => data.sources.push(data.sources[0]),
            4 => data.sources.push(data.resources.len()),
            5 => {
                data.layers.last_mut().unwrap().0 = ImageId(9);
            }
            6 => data
                .resources
                .push(Arc::new(RgbaImage::new(1, 1, vec![0; 4]).unwrap())),
            7 => data.layers.push(data.layers[0]),
            _ => data.images.last_mut().unwrap().0 = ImageId(u16::MAX),
        }
        let mut published = None;
        let result = ImageAssets::import_visual(data, ImageAssetLimits::default());
        assert!(result.is_err(), "corruption {corrupt} accepted");
        if let Ok(owner) = result {
            published = Some(owner);
        }
        assert!(published.is_none());
    }
    assert_eq!(bank.get(ImageId(0)).unwrap().pixels()[3], 255);
    assert!(ImageAssets::import_visual(bank.export_visual(), ImageAssetLimits::default()).is_ok());
}

#[test]
fn crop_only_registration_retains_original_source_dependency_and_aggregate_bytes() {
    let text = "#CANVASSIZE 3 2\n#BMP01 art.png\n#BGA03 01 0 0 1 1 0 0\n#00004:03\n";
    let chart = parse(text, Default::default()).unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.insert("chart.bms", text.as_bytes().to_vec()).unwrap();
    files.insert("art.png", raster()).unwrap();
    let bank = ImageAssets::prepare_from_source(
        &files.scope("chart.bms").unwrap(),
        &chart,
        ImageAssetLimits::default(),
    )
    .unwrap();
    assert!(bank.get(ImageId(1)).is_none());
    assert_eq!((bank.unique_images(), bank.decoded_bytes()), (1, 32));
    let data = copied_pixels(bank.export_visual());
    assert_eq!(data.sources.len(), 1);
    assert_eq!(data.source_ids, vec![(ImageId(1), Some(data.sources[0]))]);
    let receiver = ImageAssets::import_visual(data, ImageAssetLimits::default()).unwrap();
    assert_eq!(receiver.decoded_bytes(), 32);
    assert_eq!(receiver.get(ImageId(3)).unwrap().pixels().len(), 24);
    assert!(
        ImageAssets::import_visual(
            bank.export_visual(),
            ImageAssetLimits {
                max_decoded_bytes: 31,
                ..Default::default()
            }
        )
        .is_err()
    );
    // The source ID counts even though its pixels are only retained as a dependency.
    let one_identity = ImageAssetLimits {
        max_images: 1,
        ..Default::default()
    };
    assert!(
        ImageAssets::prepare_from_source(&files.scope("chart.bms").unwrap(), &chart, one_identity)
            .is_err()
    );
    assert!(ImageAssets::import_visual(bank.export_visual(), one_identity).is_err());
    let exact = ImageAssetLimits {
        max_images: 2,
        max_decoded_bytes: 32,
        ..Default::default()
    };
    assert!(
        ImageAssets::prepare_from_source(&files.scope("chart.bms").unwrap(), &chart, exact).is_ok()
    );
    assert!(ImageAssets::import_visual(bank.export_visual(), exact).is_ok());
}

#[test]
fn source_identity_aliases_reject_duplicate_missing_and_out_of_range_references() {
    let bank = prepared_images();
    let valid = bank.export_visual();
    assert!(valid.source_ids.iter().any(|(_, index)| index.is_some()));
    let variant = (0..valid.resources.len())
        .find(|index| !valid.sources.contains(index))
        .unwrap();
    for corrupt in 0..7 {
        let mut data = valid.clone();
        match corrupt {
            0 => data.source_ids.push(data.source_ids[0]),
            1 => data.source_ids.last_mut().unwrap().0 = ImageId(u16::MAX),
            2 => data.source_ids[0].1 = Some(data.resources.len()),
            3 => data.source_ids.clear(),
            4 => {
                for (_, index) in &mut data.source_ids {
                    *index = None;
                }
            }
            5 => data.source_ids[0].1 = Some(variant),
            _ => data.source_ids.swap(0, 1),
        }
        assert!(
            ImageAssets::import_visual(data, ImageAssetLimits::default()).is_err(),
            "source identity corruption {corrupt} accepted"
        );
    }
    let receiver =
        ImageAssets::import_visual(copied_pixels(valid.clone()), ImageAssetLimits::default())
            .unwrap();
    assert_eq!(receiver.export_visual().source_ids, valid.source_ids);
}

#[test]
fn unavailable_crop_sources_preserve_hidden_identity_and_original_reference_budget() {
    for (definition, reason) in [
        ("#BMP01 absent.png\n", ImageUnavailable::Missing),
        ("", ImageUnavailable::Undefined),
    ] {
        let text = format!("#CANVASSIZE 2 1\n{definition}#BGA03 01 0 0 1 1 0 0\n#00004:03\n");
        let chart = parse(&text, Default::default()).unwrap();
        let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
        files.insert("chart.bms", text.as_bytes().to_vec()).unwrap();
        let exact = ImageAssetLimits {
            max_images: 2,
            ..Default::default()
        };
        let bank =
            ImageAssets::prepare_from_source(&files.scope("chart.bms").unwrap(), &chart, exact)
                .unwrap();
        assert_eq!(bank.len(), 1);
        assert_eq!(bank.unavailable(ImageId(3)), Some(&reason));
        assert!(bank.get(ImageId(1)).is_none());
        assert_eq!(bank.decoded_bytes(), 0);
        let transfer = bank.export_visual();
        assert_eq!(transfer.source_ids, vec![(ImageId(1), None)]);
        assert!(transfer.resources.is_empty());
        assert!(transfer.sources.is_empty());
        let receiver = ImageAssets::import_visual(transfer.clone(), exact).unwrap();
        assert_eq!(receiver.unavailable(ImageId(3)), Some(&reason));
        assert_eq!(receiver.export_visual().source_ids, transfer.source_ids);
        let too_small = ImageAssetLimits {
            max_images: 1,
            ..exact
        };
        assert!(
            ImageAssets::prepare_from_source(&files.scope("chart.bms").unwrap(), &chart, too_small)
                .is_err()
        );
        assert!(ImageAssets::import_visual(transfer, too_small).is_err());
    }
}
