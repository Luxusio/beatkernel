//! Compatibility exports; canonical implementation lives under gameplay::output.
pub use crate::gameplay::output::application::owner::GameplayOutputOwner;
#[cfg(test)]
pub(crate) use crate::gameplay::output::application::owner::fixtures;
