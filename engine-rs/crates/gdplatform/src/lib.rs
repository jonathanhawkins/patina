//! # gdplatform
//!
//! Windowing, input, timing, and OS integration
//! for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod input;
pub mod os;
pub mod time;
pub mod window;

// Re-export key types at the crate root.
pub use input::{ActionBinding, InputEvent, InputMap, InputState, Key, MouseButton};
pub use os::{get_ticks_msec, get_ticks_usec, OsInfo};
pub use time::Timer;
pub use window::WindowConfig;
