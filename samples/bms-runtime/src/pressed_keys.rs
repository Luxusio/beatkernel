//! Bounded ownership of buttons admitted by the runtime, independent of judgement.
use beatkernel::input::{
    ButtonState, DeviceId, GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent,
};

const MAX_OWNERS: usize = 4096;
const VALID_MASK: u32 = (1 << 18) - 1;

/// The canonical visible BMS lane bit, excluding other game controls.
pub fn lane_bit(control: GameControlId) -> Option<u32> {
    match control.0 {
        0x11..=0x19 => Some(1 << (control.0 - 0x11)),
        0x21..=0x29 => Some(1 << (9 + control.0 - 0x21)),
        _ => None,
    }
}
/// Rejects bits outside the eighteen visible BMS lanes.
pub fn validate_mask(mask: u32) -> Result<(), String> {
    if mask & !VALID_MASK != 0 {
        Err("pressed lane mask exceeds eighteen lanes".into())
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    device: DeviceId,
    physical: PhysicalControlId,
    game: GameControlId,
}

/// At most 4096 device/control/binding owners, with retained transactional scratch.
/// Repeats do not acquire ownership and releases affect only the matching owner.
#[derive(Default)]
pub struct PressedKeys {
    owners: Vec<Owner>,
    scratch: Vec<Owner>,
    mask: u32,
}
impl PressedKeys {
    /// Applies a complete admitted batch atomically. Empty batches do no work.
    pub fn apply(&mut self, events: &[GameInputEvent]) -> Result<(), String> {
        if let [event] = events {
            let Some(bit) = lane_bit(event.game_control) else {
                return Ok(());
            };
            let PhysicalInputEvent::Button(button) = &event.physical else {
                return Ok(());
            };
            if button.state == ButtonState::Repeat {
                return Ok(());
            }
            let owner = Owner {
                device: button.meta.source,
                physical: button.control,
                game: event.game_control,
            };
            let found = self.owners.iter().position(|existing| *existing == owner);
            match (button.state, found) {
                (ButtonState::Down, None) => {
                    if self.owners.len() == MAX_OWNERS {
                        return Err("pressed ownership exceeds 4096 owners".into());
                    }
                    self.owners
                        .try_reserve_exact(MAX_OWNERS - self.owners.len())
                        .map_err(|_| "pressed ownership allocation failed")?;
                    self.owners.push(owner);
                    self.mask |= bit;
                }
                (ButtonState::Up, Some(index)) => {
                    self.owners.swap_remove(index);
                    if !self
                        .owners
                        .iter()
                        .any(|owner| owner.game == event.game_control)
                    {
                        self.mask &= !bit;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if let Some(mask) = self.prepare(events)? {
            self.commit(mask);
        }
        Ok(())
    }
    /// Current visible-lane ownership mask.
    pub fn mask(&self) -> u32 {
        self.mask
    }
    /// Releases all owners while retaining bounded reusable storage.
    pub fn clear(&mut self) {
        self.owners.clear();
        self.scratch.clear();
        self.mask = 0;
    }
    pub(crate) fn prepare(&mut self, events: &[GameInputEvent]) -> Result<Option<u32>, String> {
        if !events.iter().any(|event| matches!(&event.physical, PhysicalInputEvent::Button(button) if button.state != ButtonState::Repeat && lane_bit(event.game_control).is_some())) { return Ok(None); }
        self.scratch.clear();
        self.scratch
            .try_reserve_exact(MAX_OWNERS)
            .map_err(|_| "pressed ownership allocation failed")?;
        self.scratch.extend_from_slice(&self.owners);
        for event in events {
            if lane_bit(event.game_control).is_none() {
                continue;
            }
            let PhysicalInputEvent::Button(button) = &event.physical else {
                continue;
            };
            if button.state == ButtonState::Repeat {
                continue;
            }
            let owner = Owner {
                device: button.meta.source,
                physical: button.control,
                game: event.game_control,
            };
            let found = self.scratch.iter().position(|existing| *existing == owner);
            match (button.state, found) {
                (ButtonState::Down, None) => {
                    if self.scratch.len() == MAX_OWNERS {
                        return Err("pressed ownership exceeds 4096 owners".into());
                    }

                    self.scratch.push(owner);
                }
                (ButtonState::Up, Some(index)) => {
                    self.scratch.swap_remove(index);
                }
                _ => {}
            }
        }
        Ok(Some(self.scratch.iter().fold(0, |mask, owner| {
            mask | lane_bit(owner.game).unwrap()
        })))
    }
    pub(crate) fn commit(&mut self, mask: u32) {
        std::mem::swap(&mut self.owners, &mut self.scratch);
        self.mask = mask;
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use beatkernel::{
        input::{ButtonEvent, EventMeta},
        time::{ClockDomainId, ClockPoint, Timestamp},
    };
    pub(crate) fn button(device: u64, key: u16, game: u32, state: ButtonState) -> GameInputEvent {
        GameInputEvent {
            game_control: GameControlId(game),
            physical: PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    DeviceId(device),
                    ClockPoint {
                        domain: ClockDomainId(1),
                        timestamp: Timestamp::ZERO,
                    },
                    0,
                ),
                control: PhysicalControlId::keyboard(key),
                state,
            }),
        }
    }
    #[test]
    fn ownership_repeat_and_matching_release() {
        let mut keys = PressedKeys::default();
        keys.apply(&[button(1, 4, 0x11, ButtonState::Repeat)])
            .unwrap();
        assert_eq!(keys.mask(), 0);
        keys.apply(&[
            button(1, 4, 0x11, ButtonState::Down),
            button(2, 4, 0x11, ButtonState::Down),
            button(1, 4, 0x29, ButtonState::Down),
        ])
        .unwrap();
        assert_eq!(keys.mask(), 1 | (1 << 17));
        keys.apply(&[
            button(1, 4, 0x11, ButtonState::Up),
            button(9, 4, 0x29, ButtonState::Up),
            button(1, 4, 0x10, ButtonState::Down),
        ])
        .unwrap();
        assert_eq!(keys.mask(), 1 | (1 << 17));
        keys.apply(&[button(2, 4, 0x11, ButtonState::Up)]).unwrap();
        assert_eq!(keys.mask(), 1 << 17);
        keys.clear();
        assert_eq!(keys.mask(), 0);
    }
    #[test]
    fn distinct_keys_on_one_device_and_non_button_inputs_remain_independent() {
        use beatkernel::input::{AxisEvent, AxisMode};
        let mut keys = PressedKeys::default();
        keys.apply(&[
            button(1, 4, 0x16, ButtonState::Down),
            button(1, 5, 0x16, ButtonState::Down),
        ])
        .unwrap();
        keys.apply(&[button(1, 4, 0x16, ButtonState::Up)]).unwrap();
        assert_eq!(keys.mask(), 1 << 5);
        let PhysicalInputEvent::Button(button) = button(1, 5, 0x16, ButtonState::Up).physical
        else {
            unreachable!()
        };
        let axis = GameInputEvent {
            game_control: GameControlId(0x16),
            physical: PhysicalInputEvent::Axis(AxisEvent {
                meta: button.meta,
                control: button.control,
                value: 0.0,
                mode: AxisMode::Absolute,
            }),
        };
        let ptr = keys.owners.as_ptr();
        let capacity = keys.owners.capacity();
        keys.apply(&[axis]).unwrap();
        keys.apply(&[]).unwrap();
        assert_eq!(keys.mask(), 1 << 5);
        assert_eq!(keys.owners.as_ptr(), ptr);
        assert_eq!(keys.owners.capacity(), capacity);
        keys.apply(&[super::fixtures::button(1, 5, 0x16, ButtonState::Up)])
            .unwrap();
        assert_eq!(keys.mask(), 0);
    }
    #[test]
    fn capacity_failure_preserves_whole_batch_and_empty_storage() {
        let mut keys = PressedKeys::default();
        keys.apply(&[]).unwrap();
        assert_eq!(keys.owners.capacity(), 0);
        let events: Vec<_> = (0..MAX_OWNERS)
            .map(|i| button(i as u64, 4, 0x11, ButtonState::Down))
            .collect();
        keys.apply(&events).unwrap();
        assert!(
            keys.apply(&[
                button(0, 4, 0x11, ButtonState::Up),
                button(5000, 4, 0x29, ButtonState::Down),
                button(5001, 4, 0x29, ButtonState::Down)
            ])
            .is_err()
        );
        assert_eq!(keys.owners.len(), MAX_OWNERS);
        assert_eq!(keys.mask(), 1);
        keys.apply(&[button(0, 4, 0x11, ButtonState::Down)])
            .unwrap();
        assert_eq!(keys.owners.len(), MAX_OWNERS);
        assert!(validate_mask(1 << 18).is_err());
        assert_eq!(lane_bit(GameControlId(0x21)), Some(1 << 9));
    }
}
