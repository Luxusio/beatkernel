//! Bottom-up presentation components: atoms → molecules → organisms.
//! Each component emits geometry from explicit data; none owns gameplay or I/O.
pub mod atoms;
pub mod catalog_search;
pub mod clipboard;
pub mod devices;
pub mod display;
#[cfg(test)]
mod grapheme_editing_fixtures;
pub mod interaction;
pub mod molecules;
pub mod organisms;
pub mod players;
pub mod practice;
pub mod records;
mod retained;
pub mod selection;
pub mod settings;
pub mod text_input;
