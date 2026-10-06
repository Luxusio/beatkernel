//! Native and Player implementations of output ports.
#[cfg(target_os = "linux")]
pub mod alsa;
#[cfg(target_os = "linux")]
pub mod alsa_ui;
#[cfg(all(target_os = "windows", feature = "asio-sdk"))]
pub mod asio;
#[cfg(target_os = "macos")]
pub mod coreaudio;
#[cfg(target_os = "macos")]
pub mod coreaudio_ui;
pub mod observation;
pub mod player;
pub mod remix;
pub mod switch;
#[cfg(target_os = "windows")]
pub mod wasapi;
