// Deferred wasm32/browser child fixture; no Window or GPU is constructed.
// The current --lib compile matrix does not compile this cfg(test) module.
use super::*;
use crate::PreparedBms;
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank, VoiceId},
    input::{BackendId, ContactId, TouchEvent, TouchPhase},
    judge::JudgeStage,
    runtime::SoundBinding,
};

const SECOND: i64 = 1_000_000_000;
fn prepared(damage: &str) -> BrowserPrepared {
    let source = beatkernel_bms::parse(
        &format!("#BPM 60\n#WAV01 key\n#00011:01\n#00012:01\n#000D1:00{damage}0000\n"),
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let chart = PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
        })
        .collect();
    BrowserPrepared {
        prepared: PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands: vec![],
        },
        chart,
        visual_preview: None,
        images: Arc::new(ImageAssets::default()),
        movies: Arc::new(crate::video_assets::VideoAssets::default()),
        chart_seed: 0,
        start: Timestamp::ZERO,
        replay: None,
        play_policy: None,
    }
}
fn button(ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(HOST, ns), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn touch(ns: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(u64::MAX - 1), point(HOST, ns), sequence),
        control: PhysicalControlId::Native {
            backend: BackendId(u32::MAX),
            code: 7,
        },
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: 5.0, y: 6.0 },
        pressure: None,
    })
}

#[test]
fn browser_solo_actual_report_observation_clears_failed_owners_including_capture_error_prefix() {
    for (damage, records, expected, technical) in [
        ("ZZ", 64, 0, false),
        ("ZZ", 2, 0, true),
        ("1E", 64, 3, false),
    ] {
        let mut browser = BrowserGame::new_physical_contact(
            prepared(damage),
            0,
            0,
            0,
            0,
            0,
            vec![0x11, 0, 0, 0, 0, 7, 91, 0x12, 0, 0, 0, 1, u32::MAX, 7],
            None,
            4096,
            1024,
        )
        .unwrap();
        let limits =
            ReplayCodecLimits::new(65536, records, 4096, CodecLimits::new(4096, 1024).unwrap())
                .unwrap();
        browser.game.configure_capture(limits, 0).unwrap();
        browser.game.activate(point(HOST, 0)).unwrap();
        for input in [button(0, 1, ButtonState::Down), touch(0, 1)] {
            let result = browser
                .game
                .process_input(input, &Explicit, point(OUTPUT, 0));
            browser.accept_report(result).unwrap();
        }
        assert_eq!(browser.pressed, 3);
        assert_eq!(browser.pressed_owners.mask(), 3);
        let result = browser
            .game
            .advance_to(point(HOST, SECOND), &Explicit, point(OUTPUT, SECOND));
        if technical {
            assert!(
                matches!(&result, Err(StepGameplayError::Capture { report: Some(report), .. })
                if report.hazard_events.len() == 1)
            );
        } else {
            assert_eq!(result.as_ref().unwrap().hazard_events.len(), 1);
        }
        let hash = browser.game.judge().stable_hash().unwrap();
        assert_eq!(browser.accept_report(result).is_err(), technical);
        assert_eq!(browser.game.judge().stable_hash().unwrap(), hash);
        assert_eq!(browser.pressed, expected);
        assert_eq!(browser.pressed_owners.mask(), expected);
        assert_eq!(browser.game.gauge().snapshot().level_units, 0);
        assert_eq!(
            browser.game.gauge().snapshot().failure.is_some(),
            damage == "ZZ"
        );
        assert_eq!(browser.game.failed(), technical);
        assert_eq!(browser.recent.len(), 2); // Display cleanup adds no judge/release event.
        if !technical {
            let later = browser.game.process_input(
                button(2 * SECOND, 2, ButtonState::Up),
                &Explicit,
                point(OUTPUT, 2 * SECOND),
            );
            browser.accept_report(later).unwrap();
            assert_eq!(browser.pressed, if damage == "ZZ" { 0 } else { 2 });
            let later = browser.game.process_input(
                button(2 * SECOND, 3, ButtonState::Down),
                &Explicit,
                point(OUTPUT, 2 * SECOND),
            );
            browser.accept_report(later).unwrap();
            assert_eq!(browser.pressed, expected);
            if damage == "ZZ" {
                assert_eq!(browser.game.judge().stable_hash().unwrap(), hash);
            }
        }
    }
}
