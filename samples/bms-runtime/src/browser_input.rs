//! Bounded physical bindings and canonical input decoding for browser owners.
//! Acquisition, permissions and device report interpretation belong to adapters.
use beatkernel::{
    input::{
        BackendId, Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchRegion, TouchRouter, VendorNamespaceId,
        codec::{CodecLimits, decode_event},
    },
    time::ClockDomainId,
};

/// Setup-only mapping and byte budgets, using the same core identities as native input.
#[derive(Debug)]
pub struct PhysicalInputSetup {
    pub bindings: BindingMap,
    pub limits: CodecLimits,
}

impl PhysicalInputSetup {
    /// Each seven-word row is lane, selector, device low/high, control kind,
    /// namespace/page, code/usage. Any selectors have zero device words; exact
    /// selectors retain all 64 bits. Native/vendor control words stay intact.
    pub fn new(
        words: &[u32],
        lanes: &[u8],
        max_encoded: u32,
        max_payload: u32,
    ) -> Result<Self, String> {
        if (words.is_empty() && !lanes.is_empty()) || words.len() > 256 * 7 || words.len() % 7 != 0
        {
            return Err("physical bindings require complete seven-word rows, at most 256, and chart coverage".into());
        }
        if lanes.len() > 18 {
            return Err("prepared browser chart exceeds eighteen lanes".into());
        }
        if max_encoded > 1_048_576 {
            return Err("browser input encoded budget exceeds 1 MiB".into());
        }
        let limits = CodecLimits::new(
            usize::try_from(max_encoded).map_err(|_| "encoded input budget is unrepresentable")?,
            usize::try_from(max_payload).map_err(|_| "input payload budget is unrepresentable")?,
        )
        .map_err(|error| error.to_string())?;
        let mut bindings = BindingMap::new();
        for row in words.chunks_exact(7) {
            bindings
                .add(decode_binding(row)?)
                .map_err(|error| error.to_string())?;
        }
        for &lane in lanes {
            let game_control = GameControlId(u32::from(lane));
            if crate::pressed_keys::lane_bit(game_control).is_none()
                || !bindings
                    .bindings()
                    .iter()
                    .any(|binding| binding.game_control == game_control)
            {
                return Err("a prepared lane has no valid physical binding".into());
            }
        }
        Ok(Self { bindings, limits })
    }
}

/// Setup-only spatial routing for actual prepared lanes, using core contact owners.
#[derive(Debug)]
pub struct TouchInputSetup {
    pub router: TouchRouter,
}

impl TouchInputSetup {
    /// Seven identity words per region use the physical binding row format.
    /// Four parallel bounds are min-x/min-y/max-x/max-y in projected hit units.
    /// A subset of lanes or no regions is valid alongside ordinary key bindings.
    pub fn new(
        words: &[u32],
        bounds: &[f32],
        lanes: &[u8],
        max_contacts: u32,
    ) -> Result<Self, String> {
        if words.len() > 256 * 7 || words.len() % 7 != 0 {
            return Err("touch regions require complete seven-word rows, at most 256".into());
        }
        let count = words.len() / 7;
        if bounds.len() != count * 4 {
            return Err("touch regions require exactly four bounds per row".into());
        }
        if lanes.len() > 18
            || lanes.iter().any(|lane| {
                crate::pressed_keys::lane_bit(GameControlId(u32::from(*lane))).is_none()
            })
        {
            return Err("prepared browser chart requires at most eighteen valid lanes".into());
        }
        let mut regions = Vec::new();
        regions
            .try_reserve_exact(count)
            .map_err(|_| "touch region allocation failed")?;
        for (row, bounds) in words.chunks_exact(7).zip(bounds.chunks_exact(4)) {
            let binding = decode_binding(row)?;
            if !lanes
                .iter()
                .any(|lane| u32::from(*lane) == binding.game_control.0)
            {
                return Err("touch region destination is not a prepared lane".into());
            }
            regions.push(TouchRegion {
                device: binding.device,
                physical: binding.physical,
                game_control: binding.game_control,
                min: Position2 {
                    x: bounds[0],
                    y: bounds[1],
                },
                max: Position2 {
                    x: bounds[2],
                    y: bounds[3],
                },
            });
        }
        let max_contacts =
            usize::try_from(max_contacts).map_err(|_| "touch contact limit is unrepresentable")?;
        let router = TouchRouter::new(regions, max_contacts).map_err(|error| error.to_string())?;
        Ok(Self { router })
    }
}

// Callers admit complete seven-word rows before selecting each checked chunk.
fn decode_binding(row: &[u32]) -> Result<Binding, String> {
    let game_control = GameControlId(row[0]);
    if crate::pressed_keys::lane_bit(game_control).is_none() {
        return Err("physical binding has an invalid BMS lane".into());
    }
    let device = match row[1] {
        0 if row[2] == 0 && row[3] == 0 => DeviceSelector::Any,
        0 => return Err("Any physical binding must have zero device words".into()),
        1 => DeviceSelector::Exact(DeviceId(u64::from(row[2]) | (u64::from(row[3]) << 32))),
        _ => return Err("physical binding has an invalid device selector".into()),
    };
    let physical = decode_physical_control(row[4], row[5], row[6])?;
    Ok(Binding {
        device,
        physical,
        game_control,
    })
}

/// Decode one physical control using the shared numeric binding/profile identity.
/// HID page/usage must fit u16; native and vendor words retain all 32 bits.
pub fn decode_physical_control(
    kind: u32,
    namespace: u32,
    code: u32,
) -> Result<PhysicalControlId, String> {
    Ok(match kind {
        0 => PhysicalControlId::HidUsage {
            usage_page: u16::try_from(namespace).map_err(|_| "HID usage page exceeds u16")?,
            usage: u16::try_from(code).map_err(|_| "HID usage exceeds u16")?,
        },
        1 => PhysicalControlId::Native {
            backend: BackendId(namespace),
            code,
        },
        2 => PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(namespace),
            code,
        },
        _ => return Err("physical binding has an invalid control kind".into()),
    })
}

/// Decode one original canonical event without adapting its meaning or metadata.
/// Only the normalized acquisition clock must already be in the owner's domain;
/// optional native/original clock provenance is retained exactly as recorded.
pub fn decode_input(
    bytes: &[u8],
    limits: CodecLimits,
    host_domain: ClockDomainId,
) -> Result<PhysicalInputEvent, String> {
    let event = decode_event(bytes, limits).map_err(|error| error.to_string())?;
    if event.meta().clock_domain != host_domain || event.meta().timestamp.as_nanos() < 0 {
        return Err(
            "browser input requires a nonnegative acquisition time in its host domain".into(),
        );
    }
    Ok(event)
}
