//! Compatibility exports; canonical implementation lives under gameplay::output.
pub use crate::gameplay::output::{
    ports::OutputUiPort, application::requests::GameplayOutputUi, adapters::player::PlayerOutputUi,
};
