//! EXBMP fixtures. ARGB bytes are retained verbatim beside the shared BMP
//! image namespace; no blending or timing meaning is introduced.
use crate::{parse, parse_seeded, BgaChannel, DuplicatePolicy, ImageArgb, ImageId, ParseOptions};

fn last_wins() -> ParseOptions {
    ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    }
}
const fn argb(alpha: u8, red: u8, green: u8, blue: u8) -> ImageArgb {
    ImageArgb {
        alpha,
        red,
        green,
        blue,
    }
}

#[test]
fn exbmp_retains_path_and_exact_argb_in_shared_namespace() {
    let chart = parse(
        "#exBmP01 255,0,128,7 layer image.png\n#BMP02 plain.bmp\n#EXBMP00 0,0,0,0 poor.bmp\n#00007:0102",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(chart.images[&ImageId(1)], "layer image.png");
    assert_eq!(chart.images[&ImageId(0)], "poor.bmp");
    assert_eq!(chart.image_argb[&ImageId(1)], argb(255, 0, 128, 7));
    assert_eq!(chart.image_argb[&ImageId(0)], argb(0, 0, 0, 0));
    assert!(!chart.image_argb.contains_key(&ImageId(2)));
    assert_eq!(chart.bga.len(), 2);
    assert_eq!(chart.bga[0].channel, BgaChannel::Layer);
    assert_eq!(chart.bga[0].image, ImageId(1));
}

#[test]
fn exbmp_uses_selected_radix() {
    let chart = parse(
        "#BASE 62\n#EXBMPzZ 1,2,3,4 a.bmp\n#EXBMPZz 5,6,7,8 b.bmp",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(chart.image_argb[&ImageId(61 * 62 + 35)], argb(1, 2, 3, 4));
    assert_eq!(chart.image_argb[&ImageId(35 * 62 + 61)], argb(5, 6, 7, 8));
}

#[test]
fn strict_argb_and_path_grammar() {
    for line in [
        "#EXBMP01",
        "#EXBMP01 255,0,0,0",
        "#EXBMP01 255,0,0 a.bmp",
        "#EXBMP01 255,0,0,0,0 a.bmp",
        "#EXBMP01 256,0,0,0 a.bmp",
        "#EXBMP01 +1,0,0,0 a.bmp",
        "#EXBMP01 -1,0,0,0 a.bmp",
        "#EXBMP01 0001,0,0,0 a.bmp",
        "#EXBMP01 1,,0,0 a.bmp",
        "#EXBMP01 1, 0,0,0 a.bmp",
        "#EXBMP001 1,0,0,0 a.bmp",
        "#EXBMP!1 1,0,0,0 a.bmp",
    ] {
        assert!(parse(line, ParseOptions::default()).is_err(), "{line}");
    }
    assert_eq!(
        parse("#EXBMP01 1,0,0,0 a.bmp", ParseOptions::default())
            .unwrap()
            .image_argb[&ImageId(1)],
        argb(1, 0, 0, 0)
    );
}

#[test]
fn bmp_and_exbmp_share_duplicate_policy() {
    for source in [
        "#BMP01 a.bmp\n#EXBMP01 1,2,3,4 b.bmp",
        "#EXBMP01 1,2,3,4 b.bmp\n#BMP01 a.bmp",
        "#EXBMP01 1,2,3,4 b.bmp\n#EXBMP01 5,6,7,8 c.bmp",
    ] {
        assert_eq!(
            parse(source, ParseOptions::default()).unwrap_err().line,
            2,
            "{source}"
        );
    }
    let exbmp_last = parse("#BMP01 a.bmp\n#EXBMP01 1,2,3,4 b.bmp", last_wins()).unwrap();
    assert_eq!(exbmp_last.images[&ImageId(1)], "b.bmp");
    assert_eq!(exbmp_last.image_argb[&ImageId(1)], argb(1, 2, 3, 4));
    let bmp_last = parse("#EXBMP01 1,2,3,4 b.bmp\n#BMP01 a.bmp", last_wins()).unwrap();
    assert_eq!(bmp_last.images[&ImageId(1)], "a.bmp");
    assert!(bmp_last.image_argb.is_empty());
    let exbmp_twice = parse(
        "#EXBMP01 1,2,3,4 b.bmp\n#EXBMP01 5,6,7,8 c.bmp",
        last_wins(),
    )
    .unwrap();
    assert_eq!(exbmp_twice.images[&ImageId(1)], "c.bmp");
    assert_eq!(exbmp_twice.image_argb[&ImageId(1)], argb(5, 6, 7, 8));
}

#[test]
fn seeded_selection_and_unchanged_gameplay() {
    let conditional =
        "#RANDOM 2\n#IF 1\n#EXBMP01 malformed\n#ELSE\n#EXBMP01 9,8,7,6 a.bmp\n#ENDIF\n#ENDRANDOM";
    let selected = parse_seeded(conditional, ParseOptions::default(), 0).unwrap();
    assert_eq!(selected.image_argb[&ImageId(1)], argb(9, 8, 7, 6));
    assert!(parse_seeded(conditional, ParseOptions::default(), 3).is_err());
    let base = "#BPM 120\n#WAV01 note.wav\n#00011:01\n#00004:0100\n";
    let plain = parse(&format!("{base}#BMP01 a.bmp"), ParseOptions::default()).unwrap();
    let extended = parse(
        &format!("{base}#EXBMP01 128,0,0,0 a.bmp"),
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(plain.source, extended.source);
    assert_eq!(plain.notes, extended.notes);
    assert_eq!(plain.images, extended.images);
    assert_eq!(plain.compile().unwrap(), extended.compile().unwrap());
}
