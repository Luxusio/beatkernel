//! Deferred actual image preparation using selected in-memory files, never asset IO.
use crate::{
    asset_source::{MemoryAssetLimits, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets, ImageUnavailable, MAX_IMAGE_REFERENCES},
};
use beatkernel_bms::{BgaCrop, BmsChart, ImageId, ParseOptions, parse};
use std::sync::Arc;

fn bitmap(rgb: [u8; 3]) -> Vec<u8> {
    // Original one-pixel, uncompressed 24-bit BMP, including its padded row.
    let mut bytes = vec![0; 58];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
    bytes[54..57].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
    bytes
}

fn selected_files() -> (MemoryFiles, BmsChart) {
    let text = "#CANVASSIZE 1 1\n#BMP00 red.bmp\n#BMP0A red.bmp\n#BMP0a blue.bmp\n\
        #BGAzz 0a 0 0 1 1 0 0\n#BMPzx missing.bmp\n#BMPzw unsupported.bin\n\
        #00004:0A0azzzy\n#00006:zx\n#00007:zz\n#00104:zw\n#BASE 62";
    let chart = parse(text, ParseOptions::default()).unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", text.as_bytes().to_vec())
        .unwrap();
    files.insert("song/red.bmp", bitmap([255, 0, 0])).unwrap();
    files.insert("song/blue.bmp", bitmap([0, 0, 255])).unwrap();
    files
        .insert("song/unsupported.bin", b"not a raster".to_vec())
        .unwrap();
    (files, chart)
}

#[test]
fn selected_base62_ids_reach_actual_image_crop_and_layer_preparation_with_initial_poor_and_blank_reasons()
 {
    let (files, chart) = selected_files();
    let source = files.scope("song/chart.bms").unwrap();
    let bank =
        ImageAssets::prepare_from_source(&source, &chart, ImageAssetLimits::default()).unwrap();
    assert_eq!(MAX_IMAGE_REFERENCES, 3844);
    assert_eq!(bank.len(), 7);
    assert_eq!((bank.unique_images(), bank.decoded_bytes()), (2, 12));
    assert_eq!(bank.get(ImageId(0)).unwrap().pixels(), [255, 0, 0, 255]);
    assert_eq!(bank.get(ImageId(10)).unwrap().pixels(), [255, 0, 0, 255]);
    assert_eq!(bank.get(ImageId(36)).unwrap().pixels(), [0, 0, 255, 255]);
    assert_eq!(bank.get(ImageId(3843)).unwrap().pixels(), [0, 0, 255, 255]);
    assert!(Arc::ptr_eq(
        bank.get(ImageId(0)).unwrap(),
        bank.get(ImageId(10)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        bank.get(ImageId(3843)).unwrap(),
        bank.get_layer(ImageId(3843)).unwrap()
    ));
    assert_eq!(
        bank.unavailable(ImageId(3842)),
        Some(&ImageUnavailable::Undefined)
    );
    assert_eq!(
        bank.unavailable(ImageId(3841)),
        Some(&ImageUnavailable::Missing)
    );
    assert_eq!(
        bank.unavailable(ImageId(3840)),
        Some(&ImageUnavailable::Unsupported)
    );
    for id in [3840, 3841, 3842] {
        assert!(bank.get(ImageId(id)).is_none());
    }
    assert!(chart.source.objects.is_empty() && chart.notes.is_empty() && chart.bgm.is_empty());
}

#[test]
fn high_resource_ids_do_not_relax_reference_encoded_decoded_or_generic_identity_bounds() {
    let (files, chart) = selected_files();
    let source = files.scope("song/chart.bms").unwrap();
    let exact = ImageAssetLimits {
        max_images: 7,
        max_decoded_bytes: 12,
        ..ImageAssetLimits::default()
    };
    let bank = ImageAssets::prepare_from_source(&source, &chart, exact).unwrap();
    assert_eq!(bank.decoded_bytes(), 12);
    assert!(
        ImageAssets::prepare_from_source(
            &source,
            &chart,
            ImageAssetLimits {
                max_images: 6,
                ..exact
            }
        )
        .is_err()
    );
    assert!(
        ImageAssets::prepare_from_source(
            &source,
            &chart,
            ImageAssetLimits {
                max_decoded_bytes: 11,
                ..exact
            }
        )
        .is_err()
    );
    let mut encoded = exact;
    encoded.decode.max_encoded_bytes = 57;
    assert!(ImageAssets::prepare_from_source(&source, &chart, encoded).is_err());
    encoded.decode.max_encoded_bytes = 58;
    assert_eq!(
        ImageAssets::prepare_from_source(&source, &chart, encoded)
            .unwrap()
            .decoded_bytes(),
        12
    );
    for max_images in [0, 3845] {
        assert!(
            ImageAssetLimits {
                max_images,
                ..exact
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        ImageAssetLimits {
            max_images: 3844,
            ..exact
        }
        .validate()
        .is_ok()
    );
    let mut invalid_selection = chart.clone();
    invalid_selection.bga[0].image = ImageId(3844);
    assert!(
        ImageAssets::prepare_from_source(&source, &invalid_selection, ImageAssetLimits::default())
            .is_err()
    );
    let mut invalid_destination = chart.clone();
    invalid_destination.bga_crops.insert(
        ImageId(3844),
        BgaCrop {
            source: ImageId(36),
            source_rect: [0, 0, 1, 1],
            destination: [0, 0],
        },
    );
    assert!(
        ImageAssets::prepare_from_source(
            &source,
            &invalid_destination,
            ImageAssetLimits::default()
        )
        .is_err()
    );
    let mut invalid_source = chart.clone();
    invalid_source
        .bga_crops
        .get_mut(&ImageId(3843))
        .unwrap()
        .source = ImageId(3844);
    assert!(
        ImageAssets::prepare_from_source(&source, &invalid_source, ImageAssetLimits::default())
            .is_err()
    );
    // Rejected preparations do not mutate either the admitted chart or bank.
    assert_eq!(chart.bga_crops[&ImageId(3843)].source, ImageId(36));
    assert_eq!(bank.get(ImageId(3843)).unwrap().pixels(), [0, 0, 255, 255]);
}
