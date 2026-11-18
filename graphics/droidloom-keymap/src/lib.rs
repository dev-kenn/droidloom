//! Keyboard and mouse to touch mapping for Droidloom game windows.
//!
//! The crate has no Wayland or Android dependencies. A host presenter feeds it
//! evdev key and button codes and gets back plain touch records with
//! coordinates normalised to the window (0.0 to 1.0). Coordinates are exactly
//! the ones in the profile: the mapper adds no offset, jitter or timing change.
//!
//! Profiles are plain TOML files, one per Android package. See
//! [`Profile::parse`] for the format.

#![forbid(unsafe_code)]

mod keys;
mod mapper;
mod profile;

pub use keys::parse_key;
pub use mapper::{
    InputState, MAX_CONTACTS, Mapper, Outcome, Phase, SYNTHETIC_POINTER_BASE, TouchOut,
};
pub use profile::{Profile, ProfileError, Tap};
