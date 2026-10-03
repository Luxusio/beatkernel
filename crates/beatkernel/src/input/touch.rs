//! Bounded region selection that keeps a contact's initial logical destination.

use std::fmt;

use super::{
    ContactId, DeviceId, DeviceSelector, GameControlId, GameInputEvent, PhysicalControlId,
    PhysicalInputEvent, Position2, TouchPhase,
};

const MAX_REGIONS: usize = 256;
const MAX_CONTACTS: usize = 4096;

/// A half-open touch rectangle in the acquisition adapter's coordinate units.
///
/// Exact device/surface regions override `Any` regions for the entire surface,
/// including gaps between exact regions. Bounds are fixed for the router's life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchRegion {
    /// The source selection policy.
    pub device: DeviceSelector,
    /// The complete physical touch surface identity.
    pub physical: PhysicalControlId,
    /// The logical destination selected by an initial Down inside this region.
    pub game_control: GameControlId,
    /// Inclusive lower x/y bounds, both finite.
    pub min: Position2,
    /// Exclusive upper x/y bounds, finite and strictly above `min` on both axes.
    pub max: Position2,
}

/// Routing result for one unchanged physical sample.
#[derive(Clone, Debug, PartialEq)]
pub enum TouchRoute {
    /// Not a touch sample, or its source/surface has no applicable region rules.
    /// The caller may use its ordinary binding path; the router did not mutate.
    Unconfigured,
    /// A configured surface has no destination for this contact.
    /// This must not fall through to ordinary surface bindings.
    Ignored,
    /// The initial Down's destination with this sample's original payload.
    Bound(GameInputEvent),
}

/// Rejected setup or sample; routing state remains unchanged on every error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchRoutingError {
    /// Region count exceeds the fixed maximum of 256.
    InvalidRegionLimit {
        /// Requested number of regions.
        count: usize,
    },
    /// Contact capacity lies outside 1 through 4096.
    InvalidContactLimit {
        /// Requested simultaneous contact capacity.
        max_contacts: usize,
    },
    /// A region contains nonfinite coordinates or lacks positive x/y extent.
    InvalidRegion {
        /// Invalid region's index in the supplied order.
        index: usize,
    },
    /// Two rectangles overlap for the same source selector and physical surface.
    OverlappingRegions {
        /// Earlier region's index in the supplied order.
        first: usize,
        /// Later region's index in the supplied order.
        second: usize,
    },
    /// A configured sample has a nonfinite coordinate or supplied pressure.
    NonFiniteSample,
    /// A new Down cannot fit; existing contacts may still move or release.
    ContactCapacity,
    /// Contact storage could not be reserved during construction.
    AllocationFailed,
}

impl fmt::Display for TouchRoutingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRegionLimit { count } => {
                write!(
                    formatter,
                    "touch region count {count} exceeds {MAX_REGIONS}"
                )
            }
            Self::InvalidContactLimit { max_contacts } => {
                write!(
                    formatter,
                    "touch contact capacity {max_contacts} must be 1..={MAX_CONTACTS}"
                )
            }
            Self::InvalidRegion { index } => {
                write!(
                    formatter,
                    "touch region {index} requires finite positive bounds"
                )
            }
            Self::OverlappingRegions { first, second } => {
                write!(formatter, "touch regions {first} and {second} overlap")
            }
            Self::NonFiniteSample => {
                write!(formatter, "touch position and pressure must be finite")
            }
            Self::ContactCapacity => write!(formatter, "touch contact capacity exhausted"),
            Self::AllocationFailed => write!(formatter, "touch contact storage allocation failed"),
        }
    }
}

impl std::error::Error for TouchRoutingError {}

#[derive(Clone, Copy, Debug)]
struct ActiveContact {
    source: DeviceId,
    physical: PhysicalControlId,
    contact: ContactId,
    destination: Option<GameControlId>,
}

/// Caller-owned spatial routing with bounded, preallocated contact ownership.
///
/// The first Down retains its destination (including no destination) until Up or
/// Cancel. Device, surface and contact all identify an owner. Duplicate Down and
/// movement cannot switch lanes. Configured samples keep their physical variant,
/// coordinates, pressure, timestamps, sequence and acquisition provenance.
///
/// Setup checks at most 256 regions pairwise and reserves contact storage once.
/// Routing uses bounded linear searches and does not allocate. This component
/// neither judges nor normalizes clocks or validates chronological order. A
/// runtime must admit timing before routing and coordinate routing ownership
/// when restoring or replacing gameplay state; no implicit restore is provided.
#[derive(Debug)]
pub struct TouchRouter {
    regions: Vec<TouchRegion>,
    max_contacts: usize,
    contacts: Vec<ActiveContact>,
}

impl TouchRouter {
    /// Fallibly copies configuration and held destinations, including unbound contacts.
    /// The copy reserves the full contact limit, so routing never grows storage.
    /// Callers must pair this routing checkpoint with the matching judge/transport
    /// state; this method does not validate or restore another gameplay owner.
    pub fn try_clone(&self) -> Result<Self, TouchRoutingError> {
        let mut regions = Vec::new();
        regions
            .try_reserve_exact(self.regions.len())
            .map_err(|_| TouchRoutingError::AllocationFailed)?;
        regions.extend_from_slice(&self.regions);
        let mut contacts = Vec::new();
        contacts
            .try_reserve_exact(self.max_contacts)
            .map_err(|_| TouchRoutingError::AllocationFailed)?;
        contacts.extend_from_slice(&self.contacts);
        Ok(Self {
            regions,
            max_contacts: self.max_contacts,
            contacts,
        })
    }

    /// Validates fixed regions and reserves all simultaneous contact slots.
    /// Empty regions are valid; overlapping `Any`/`Exact` regions are permitted.
    /// Same-selector/surface regions may touch edges but must not overlap.
    pub fn new(regions: Vec<TouchRegion>, max_contacts: usize) -> Result<Self, TouchRoutingError> {
        if regions.len() > MAX_REGIONS {
            return Err(TouchRoutingError::InvalidRegionLimit {
                count: regions.len(),
            });
        }
        if !(1..=MAX_CONTACTS).contains(&max_contacts) {
            return Err(TouchRoutingError::InvalidContactLimit { max_contacts });
        }
        for (index, region) in regions.iter().enumerate() {
            if !finite(region.min)
                || !finite(region.max)
                || region.min.x >= region.max.x
                || region.min.y >= region.max.y
            {
                return Err(TouchRoutingError::InvalidRegion { index });
            }
            for (first, other) in regions[..index].iter().enumerate() {
                if region.device == other.device
                    && region.physical == other.physical
                    && region.min.x < other.max.x
                    && other.min.x < region.max.x
                    && region.min.y < other.max.y
                    && other.min.y < region.max.y
                {
                    return Err(TouchRoutingError::OverlappingRegions {
                        first,
                        second: index,
                    });
                }
            }
        }
        let mut contacts = Vec::new();
        contacts
            .try_reserve_exact(max_contacts)
            .map_err(|_| TouchRoutingError::AllocationFailed)?;
        Ok(Self {
            regions,
            max_contacts,
            contacts,
        })
    }

    /// Routes one sample, preserving first-Down ownership and the full payload.
    ///
    /// Unconfigured samples do not change state or undergo coordinate validation.
    /// Configured positions and optional pressure must be finite before any
    /// mutation; finite pressure has no imposed range. Outside Down still occupies
    /// a contact slot, preventing later movement or duplicate Down from entering
    /// a region. Unknown Move/Up/Cancel are ignored without binding fallback.
    pub fn route(&mut self, event: &PhysicalInputEvent) -> Result<TouchRoute, TouchRoutingError> {
        let PhysicalInputEvent::Touch(touch) = event else {
            return Ok(TouchRoute::Unconfigured);
        };
        self.route_at(event, touch.position)
    }

    /// Routes using a separately projected point without editing physical input.
    ///
    /// The projected point selects the region only for a fresh Down; later
    /// samples retain the initial destination. Configured samples require both
    /// original and projected positions and optional pressure to be finite,
    /// including duplicate Down and releases. Unconfigured samples ignore the
    /// projection. Returned samples retain their original position and units.
    pub fn route_at(
        &mut self,
        event: &PhysicalInputEvent,
        position: Position2,
    ) -> Result<TouchRoute, TouchRoutingError> {
        let PhysicalInputEvent::Touch(touch) = event else {
            return Ok(TouchRoute::Unconfigured);
        };
        let exact = DeviceSelector::Exact(touch.meta.source);
        let selector =
            if self
                .regions
                .iter()
                .any(|region| region.device == exact && region.physical == touch.control)
            {
                exact
            } else if self.regions.iter().any(|region| {
                region.device == DeviceSelector::Any && region.physical == touch.control
            }) {
                DeviceSelector::Any
            } else {
                return Ok(TouchRoute::Unconfigured);
            };
        if !finite(touch.position)
            || !finite(position)
            || touch.pressure.is_some_and(|pressure| !pressure.is_finite())
        {
            return Err(TouchRoutingError::NonFiniteSample);
        }
        let existing = self.contacts.iter().position(|owner| {
            owner.source == touch.meta.source
                && owner.physical == touch.control
                && owner.contact == touch.contact
        });
        let destination = match (touch.phase, existing) {
            (TouchPhase::Down | TouchPhase::Move, Some(index)) => self.contacts[index].destination,
            (TouchPhase::Up | TouchPhase::Cancel, Some(index)) => {
                self.contacts.swap_remove(index).destination
            }
            (TouchPhase::Down, None) => {
                if self.contacts.len() == self.max_contacts {
                    return Err(TouchRoutingError::ContactCapacity);
                }
                let destination = self
                    .regions
                    .iter()
                    .find(|region| {
                        region.device == selector
                            && region.physical == touch.control
                            && position.x >= region.min.x
                            && position.x < region.max.x
                            && position.y >= region.min.y
                            && position.y < region.max.y
                    })
                    .map(|region| region.game_control);
                self.contacts.push(ActiveContact {
                    source: touch.meta.source,
                    physical: touch.control,
                    contact: touch.contact,
                    destination,
                });
                destination
            }
            (TouchPhase::Move | TouchPhase::Up | TouchPhase::Cancel, None) => {
                return Ok(TouchRoute::Ignored);
            }
        };
        Ok(match destination {
            Some(game_control) => TouchRoute::Bound(GameInputEvent {
                game_control,
                // Touch is a fixed-size semantic payload, so this clone cannot
                // allocate even though other physical variants own byte vectors.
                physical: PhysicalInputEvent::Touch(touch.clone()),
            }),
            None => TouchRoute::Ignored,
        })
    }

    /// Returns immutable region configuration in the supplied order.
    pub fn regions(&self) -> &[TouchRegion] {
        &self.regions
    }

    /// Returns the admitted simultaneous contact limit.
    pub fn max_contacts(&self) -> usize {
        self.max_contacts
    }

    /// Returns held contacts, including contacts whose initial Down was outside.
    pub fn active_contacts(&self) -> usize {
        self.contacts.len()
    }

    /// Removes routing ownership and returns its previous count, retaining storage.
    /// This does not synthesize Cancel, release judge holds or complete gameplay.
    /// The caller must coordinate cancellation or a fresh session separately.
    pub fn clear(&mut self) -> usize {
        let count = self.contacts.len();
        self.contacts.clear();
        count
    }
}

fn finite(position: Position2) -> bool {
    position.x.is_finite() && position.y.is_finite()
}
