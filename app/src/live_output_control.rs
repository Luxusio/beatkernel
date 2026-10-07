//! Compatibility exports; canonical implementation lives under gameplay::output.
pub use crate::gameplay::output::domain::control::{
    OutputCapability, OutputRequest, OutputReply, OutputControls,
};
#[cfg(test)]
pub(crate) use crate::gameplay::output::domain::control::fixtures;
