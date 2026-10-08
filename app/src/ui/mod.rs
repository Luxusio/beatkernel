//! Bottom-up presentation components: atoms → molecules → organisms.
//! Each component emits geometry from explicit data; none owns gameplay or I/O.
pub mod atoms;
pub mod catalog_search;
pub mod clipboard;
pub mod devices;
pub mod display;
#[cfg(test)]
mod grapheme_editing_fixtures;
#[cfg(test)]
mod grapheme_window_fixtures;
pub mod interaction;
pub mod layout;
pub mod molecules;
pub mod motion;
pub mod organisms;
pub mod players;
pub mod practice;
pub mod records;
pub mod results;
mod retained;
pub mod selection;
pub mod settings;
pub mod text_input;

#[cfg(test)]
mod browser_local_projection_fixtures;
pub mod live_audio;
