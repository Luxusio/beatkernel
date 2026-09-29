use std::fmt;

use super::{DeviceId, PhysicalControlId, PhysicalInputEvent};

/// A caller-defined logical game control or stream channel.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GameControlId(
    /// The caller-assigned logical identity.
    pub u32,
);

/// Selects the runtime source of a physical control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSelector {
    /// Applies when no exact rule matches the source and physical control.
    Any,
    /// Applies only to this runtime device and overrides matching `Any` rules.
    Exact(DeviceId),
}

/// Maps a complete physical identity to a logical destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    /// The source selection policy.
    pub device: DeviceSelector,
    /// The complete physical control identity.
    pub physical: PhysicalControlId,
    /// The logical game control or channel receiving the unchanged sample.
    pub game_control: GameControlId,
}

/// A rejected binding configuration edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingError {
    /// This selector, physical control and destination triple already exists.
    Duplicate(Binding),
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(binding) => write!(formatter, "duplicate binding: {binding:?}"),
        }
    }
}

impl std::error::Error for BindingError {}

/// An owned logical destination with its unchanged physical sample.
///
/// This value can outlive the input sample and map that produced it. Touch,
/// pointer and pose samples keep their full payload rather than becoming buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct GameInputEvent {
    /// The selected logical game control or stream channel.
    pub game_control: GameControlId,
    /// The complete input, including source, physical identity and provenance.
    pub physical: PhysicalInputEvent,
}

/// An ordered, caller-owned physical-to-game binding configuration.
///
/// Mapping is stateless: edits can change destinations of later samples in an
/// active stream. Consumers coordinate such changes with held gameplay state.
/// Configuration edits may allocate; construction through validated adds is
/// quadratic. Mapping uses at most two linear passes and does not allocate with
/// the current fixed-size semantic event variants. No measured latency or
/// audio-callback suitability is implied.
///
/// ```
/// use beatkernel::input::{Binding, BindingMap, DeviceSelector, GameControlId,
///     PhysicalControlId};
/// let rule = Binding { device: DeviceSelector::Any,
///     physical: PhysicalControlId::keyboard(0x04), game_control: GameControlId(1) };
/// let mut map = BindingMap::from_bindings([rule])?;
/// assert_eq!(map.bindings(), &[rule]);
/// assert!(map.remove(&rule));
/// # Ok::<(), beatkernel::input::BindingError>(())
/// ```
#[derive(Debug, Default)]
pub struct BindingMap {
    bindings: Vec<Binding>,
}

impl BindingMap {
    /// Constructs an empty binding configuration.
    pub const fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Constructs ordered bindings, rejecting duplicate triples.
    ///
    /// On error no partially constructed map is returned.
    pub fn from_bindings(
        bindings: impl IntoIterator<Item = Binding>,
    ) -> Result<Self, BindingError> {
        let mut map = Self::new();
        for binding in bindings {
            map.add(binding)?;
        }
        Ok(map)
    }

    /// Appends a rule, rejecting an identical triple without changing the map.
    ///
    /// Distinct destinations for the same selector/control are valid fanout.
    pub fn add(&mut self, binding: Binding) -> Result<(), BindingError> {
        if self.bindings.contains(&binding) {
            return Err(BindingError::Duplicate(binding));
        }
        self.bindings.push(binding);
        Ok(())
    }

    /// Removes a rule while preserving remaining order; false means absent.
    pub fn remove(&mut self, binding: &Binding) -> bool {
        if let Some(index) = self.bindings.iter().position(|stored| stored == binding) {
            self.bindings.remove(index);
            true
        } else {
            false
        }
    }

    /// Returns the immutable rules in insertion order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Maps a semantic input to owned destinations in insertion order.
    ///
    /// Exact source/control rules suppress matching `Any` rules. Other exact
    /// rules do not affect fallback. All physical payload and metadata bits are
    /// retained. Raw HID and custom payloads have no semantic control identity,
    /// so they yield nothing; interpret them with a device adapter first.
    /// Only the iterator borrows this map and input, not its yielded values.
    pub fn map<'a>(
        &'a self,
        event: &'a PhysicalInputEvent,
    ) -> impl Iterator<Item = GameInputEvent> + 'a {
        let control = semantic_control(event);
        let exact = DeviceSelector::Exact(event.meta().source);
        let selector = if self
            .bindings
            .iter()
            .any(|binding| binding.device == exact && Some(binding.physical) == control)
        {
            exact
        } else {
            DeviceSelector::Any
        };
        self.bindings
            .iter()
            .filter(move |binding| binding.device == selector && Some(binding.physical) == control)
            .map(move |binding| GameInputEvent {
                game_control: binding.game_control,
                physical: event.clone(),
            })
    }
}

fn semantic_control(event: &PhysicalInputEvent) -> Option<PhysicalControlId> {
    match event {
        PhysicalInputEvent::Button(event) => Some(event.control),
        PhysicalInputEvent::Axis(event) => Some(event.control),
        PhysicalInputEvent::Touch(event) => Some(event.control),
        PhysicalInputEvent::Pointer(event) => Some(event.control),
        PhysicalInputEvent::Pose(event) => Some(event.control),
        PhysicalInputEvent::RawHidReport(_) | PhysicalInputEvent::Custom(_) => None,
    }
}
