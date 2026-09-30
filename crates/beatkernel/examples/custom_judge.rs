//! Caller-owned candidate selection and grading through the public judge API.
//!
//! Run: `cargo run -p beatkernel --example custom_judge -- [--help]`.
//! This finite virtual-input fixture compares default closest-target selection
//! with later-target priority. Both judges receive the same canonical bound key
//! at song 510ms, with eligible targets at 500ms and 520ms. No native acquisition,
//! audio output, physical timing or replay support is claimed by this example.

use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceCapabilities, DeviceDescriptor,
        DeviceId, DeviceSelector, DeviceTransport, EventMeta, GameControlId, PhysicalControlId,
        PhysicalInputEvent, VirtualInputBackend,
    },
    interaction::InstantEvaluator,
    judge::{
        Candidate, CandidateResolver, JudgeEngine, JudgeEvent, JudgeGrade, JudgePolicy,
        JudgeProfile, JudgeWindow, Rule,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::error::Error;

const HOST: ClockDomainId = ClockDomainId(1);
const DEVICE: DeviceId = DeviceId(1);
const HOST_ORIGIN: i64 = 1_000_000_000;

struct SameDomainOnly;
impl ClockMapper for SameDomainOnly {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        // VirtualInputBackend already handles points in its own clock domain.
        // No relationship with another clock is inferred.
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

struct LaterTarget;
impl CandidateResolver for LaterTarget {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        // Select only from the engine's eligible candidates, preserving its
        // activation/window checks. Equal targets resolve by smaller object ID.
        candidates
            .iter()
            .min_by_key(|candidate| (std::cmp::Reverse(candidate.target), candidate.object))
            .map(|candidate| candidate.object)
    }
}

struct DirectionalGrade;
impl JudgePolicy for DirectionalGrade {
    fn grade(&self, delta: i128, profile: &JudgeProfile) -> Option<JudgeGrade> {
        // Keep the caller's inclusive timing windows, but replace their grade
        // labels with early=70 and on-time/late=71. Delta stays in integer ns.
        profile.grade(delta)?;
        Some(JudgeGrade(if delta < 0 { 70 } else { 71 }))
    }
}
// These stateless policies use the default unsupported snapshot hooks. A caller
// that needs replay checkpoints must also provide complete snapshot identities
// and clones; changing candidate priority alone does not grant replay support.

fn build(custom: bool) -> Result<JudgeEngine, Box<dyn Error>> {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1)?)?;
    for (id, tick) in [(1, 500), (2, 520)] {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(tick)?,
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(1),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let rules = vec![Rule {
        interaction: InteractionId(1),
        control: GameControlId(1),
        evaluator: Box::new(InstantEvaluator),
    }];
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(100_000_000),
            late: Duration::from_nanos(100_000_000),
        }],
        Duration::ZERO,
    )?;
    Ok(if custom {
        JudgeEngine::with_policies(
            source.compile()?,
            rules,
            profile,
            Box::new(LaterTarget),
            Box::new(DirectionalGrade),
        )?
    } else {
        JudgeEngine::new(source.compile()?, rules, profile)?
    })
}

fn print_events(label: &str, events: Vec<JudgeEvent>) {
    for event in events {
        println!("{label}: {event:?}");
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("cargo run -p beatkernel --example custom_judge -- [--help]\nFinite software fixture: default closest target versus custom later target and directional grades. Uses a virtual canonical key, explicit device binding and host-to-song transport. No native input/audio or replay capture.");
        return Ok(());
    }
    if !args.is_empty() {
        return Err("expected no arguments or --help".into());
    }
    let mut ordinary = build(false)?;
    let mut custom = build(true)?;
    let mut input = VirtualInputBackend::new(HOST);
    input.register_device(DeviceDescriptor {
        runtime_id: DEVICE,
        vendor_id: None,
        product_id: None,
        serial: None,
        name: Some("virtual policy keyboard".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            button: true,
            ..Default::default()
        },
    })?;
    let key = PhysicalControlId::keyboard(0x04);
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DEVICE),
        physical: key,
        game_control: GameControlId(1),
    }])?;
    let transport = Transport::new(
        Timestamp::from_nanos(HOST_ORIGIN),
        Timestamp::ZERO,
        Rate::NORMAL,
    );
    input.push(
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DEVICE,
                ClockPoint {
                    domain: HOST,
                    timestamp: Timestamp::from_nanos(HOST_ORIGIN + 510_000_000),
                },
                1,
            ),
            control: key,
            state: ButtonState::Down,
        }),
        &SameDomainOnly,
    )?;
    println!("Software fixture: closest chooses object1/+10ms; later-target priority chooses object2/-10ms with custom early grade70.");
    for physical in input.drain_events() {
        for bound in bindings.map(&physical) {
            let song_time = transport.position_at(bound.physical.meta().timestamp)?;
            print_events("default", ordinary.push_input(&bound, song_time)?);
            print_events("custom", custom.push_input(&bound, song_time)?);
        }
    }
    let finish = transport.position_at(Timestamp::from_nanos(HOST_ORIGIN + 621_000_000))?;
    // Advancing the actual judges shows the unselected object expiring; there
    // is no synthesized judge output or attempt to grade both targets at once.
    print_events("default remaining", ordinary.advance_to(finish)?);
    print_events("custom remaining", custom.advance_to(finish)?);
    Ok(())
}
