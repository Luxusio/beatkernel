// Deferred wasm32/browser child fixture. Real local owner/feedback paths only;
// the current --lib compile matrix does not compile this cfg(test) module.
use super::*;
use crate::PreparedBms;
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank, VoiceId},
    input::{
        BackendId, ButtonEvent, ButtonState, ContactId, EventMeta, PhysicalControlId, TouchEvent,
        TouchPhase,
    },
    judge::JudgeStage,
    runtime::SoundBinding,
};

const SECOND: i64 = 1_000_000_000;
const IDS: [PlayerId; 2] = [PlayerId(7), PlayerId(u32::MAX)];
const SOURCES: [u64; 2] = [u64::MAX, u64::MAX - 1];
fn prepared() -> BrowserPrepared {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 key\n#00011:01\n#00012:01\n#000D1:00ZZ0000\n",
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
fn button(member: usize, ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(SOURCES[member]), point(HOST, ns), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn touch(member: usize, ns: i64, sequence: u64, id: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(SOURCES[member]), point(HOST, ns), sequence),
        control: PhysicalControlId::Native {
            backend: BackendId(u32::MAX),
            code: 7,
        },
        contact: ContactId(id),
        phase,
        position: Position2 { x: 5.0, y: 6.0 },
        pressure: None,
    })
}
fn feed(browser: &mut BrowserLocalGame, event: PhysicalInputEvent) {
    let song = event.meta().timestamp.as_nanos();
    let InputResult::Processed(reports) = browser
        .game
        .process_input(event, &Explicit, point(OUTPUT, song))
        .unwrap()
    else {
        panic!("admitted local source must produce reports");
    };
    browser.accept_reports(Ok(reports)).unwrap();
}

#[test]
fn browser_local_failed_feedback_preserves_other_players_and_observes_technical_prefix() {
    for capture_failure in [false, true] {
        let plan = IDS
            .iter()
            .zip(SOURCES)
            .flat_map(|(id, source)| [id.0, 1, source as u32, (source >> 32) as u32])
            .collect();
        let mut bindings = Vec::new();
        for (id, source) in IDS.iter().zip(SOURCES) {
            bindings.extend([
                id.0,
                0x11,
                1,
                source as u32,
                (source >> 32) as u32,
                0,
                7,
                91,
            ]);
            bindings.extend([
                id.0,
                0x12,
                1,
                source as u32,
                (source >> 32) as u32,
                1,
                u32::MAX,
                7,
            ]);
        }
        let mut browser = BrowserLocalGame::new_physical(
            prepared(),
            0,
            0,
            0,
            0,
            0,
            plan,
            bindings,
            None,
            true,
            4096,
            1024,
        )
        .unwrap();
        for (member, &player) in IDS.iter().enumerate() {
            let count = if capture_failure && member == 0 {
                2
            } else {
                64
            };
            browser
                .game
                .configure_capture(
                    player,
                    ReplayCodecLimits::new(
                        65536,
                        count,
                        4096,
                        CodecLimits::new(4096, 1024).unwrap(),
                    )
                    .unwrap(),
                    0,
                )
                .unwrap();
        }
        browser.game.activate(point(HOST, 0)).unwrap();
        for member in 0..2 {
            feed(&mut browser, button(member, 0, 1, ButtonState::Down));
            feed(
                &mut browser,
                touch(member, 0, 2, u64::MAX, TouchPhase::Down),
            );
        }
        assert_eq!(
            browser
                .members
                .iter()
                .map(|member| member.pressed)
                .collect::<Vec<_>>(),
            [3, 3]
        );
        feed(&mut browser, button(1, SECOND / 2, 3, ButtonState::Up));
        let result = browser
            .game
            .advance_to(point(HOST, SECOND), &Explicit, point(OUTPUT, SECOND));
        if capture_failure {
            assert!(
                matches!(&result, Err(StepLocalGameplayError::Operation { reports, member_errors, .. })
                if reports.len() == 2 && member_errors.len() == 1)
            );
        } else {
            assert_eq!(result.as_ref().unwrap().len(), 2);
        }
        let failed_hash = browser.game.judge(IDS[0]).unwrap().stable_hash().unwrap();
        assert_eq!(browser.accept_reports(result).is_err(), capture_failure);
        assert_eq!(
            browser
                .members
                .iter()
                .map(|member| member.pressed)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(browser.members[0].pressed_owners.mask(), 0);
        assert_eq!(browser.members[1].pressed_owners.mask(), 2);
        assert!(
            browser
                .game
                .gauge(IDS[0])
                .unwrap()
                .snapshot()
                .failure
                .is_some()
        );
        assert!(
            browser
                .game
                .gauge(IDS[1])
                .unwrap()
                .snapshot()
                .failure
                .is_none()
        );
        assert_eq!(browser.game.failed(), capture_failure);
        assert_eq!(
            browser.game.judge(IDS[0]).unwrap().stable_hash().unwrap(),
            failed_hash
        );
        assert!(
            browser
                .members
                .iter()
                .all(|member| member.recent.len() == 2)
        );
        if !capture_failure {
            feed(
                &mut browser,
                touch(0, 2 * SECOND, 3, u64::MAX - 1, TouchPhase::Down),
            );
            assert_eq!(browser.members[0].pressed, 0);
            assert_eq!(
                browser.game.judge(IDS[0]).unwrap().stable_hash().unwrap(),
                failed_hash
            );
            feed(
                &mut browser,
                touch(1, 2 * SECOND, 4, u32::MAX as u64, TouchPhase::Cancel),
            );
            assert_eq!(browser.members[1].pressed, 2); // A different full-width contact does not release it.
            feed(&mut browser, button(1, 2 * SECOND, 5, ButtonState::Down));
            assert_eq!(browser.members[1].pressed, 3);
            feed(
                &mut browser,
                touch(1, 2 * SECOND, 6, u64::MAX, TouchPhase::Cancel),
            );
            assert_eq!(browser.members[1].pressed, 1);
            assert_eq!(browser.game.song_time(), Timestamp::from_nanos(2 * SECOND));
        }
    }
}
