//! Per-game profile files.

use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;

use crate::keys::parse_key;

/// A profile could not be read or failed validation.
#[derive(Debug, Error, PartialEq)]
pub enum ProfileError {
    /// The TOML text is malformed or has unknown fields.
    #[error("profile is not valid: {0}")]
    Syntax(String),
    /// The `package` field is empty.
    #[error("profile has no package name")]
    EmptyPackage,
    /// A key or button name is not known.
    #[error("unknown key or button name: {0}")]
    UnknownKey(String),
    /// A coordinate is outside the window.
    #[error("tap for {key} has coordinates outside 0.0 to 1.0")]
    OutOfRange {
        /// The key the tap belongs to, as written in the file.
        key: String,
    },
    /// The same key or button is mapped twice, or is also the toggle key.
    #[error("key or button is used twice: {0}")]
    DuplicateKey(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    package: String,
    toggle_key: Option<String>,
    #[serde(default)]
    tap: Vec<RawTap>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTap {
    key: String,
    x: f32,
    y: f32,
}

/// One key mapped to a fixed point in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    /// Linux evdev key or button code.
    pub key: u16,
    /// Horizontal position, 0.0 (left) to 1.0 (right).
    pub x: f32,
    /// Vertical position, 0.0 (top) to 1.0 (bottom).
    pub y: f32,
}

/// A validated keymap for one Android package.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Android package the profile applies to.
    pub package: String,
    /// Optional key that switches the mapper on and off.
    pub toggle_key: Option<u16>,
    /// Key to point mappings.
    pub taps: Vec<Tap>,
}

impl Profile {
    /// Parse and validate a profile from TOML text.
    ///
    /// ```toml
    /// package = "com.example.game"
    /// toggle_key = "F8"        # optional: turns the mapper on and off
    ///
    /// [[tap]]
    /// key = "R"                # a key name, a number or "code:N"
    /// x = 0.82                 # 0.0 (left) to 1.0 (right)
    /// y = 0.64                 # 0.0 (top) to 1.0 (bottom)
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`ProfileError`] for malformed text, an unknown key name,
    /// coordinates outside the window or a key used twice.
    pub fn parse(text: &str) -> Result<Self, ProfileError> {
        let raw: RawProfile =
            toml::from_str(text).map_err(|error| ProfileError::Syntax(error.to_string()))?;
        if raw.package.trim().is_empty() {
            return Err(ProfileError::EmptyPackage);
        }
        let mut used = BTreeSet::new();
        let toggle_key = match raw.toggle_key {
            Some(name) => {
                let code = parse_key(&name).ok_or(ProfileError::UnknownKey(name.clone()))?;
                used.insert(code);
                Some(code)
            }
            None => None,
        };
        let mut taps = Vec::with_capacity(raw.tap.len());
        for tap in raw.tap {
            let key = parse_key(&tap.key).ok_or(ProfileError::UnknownKey(tap.key.clone()))?;
            let in_range = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
            if !in_range(tap.x) || !in_range(tap.y) {
                return Err(ProfileError::OutOfRange { key: tap.key });
            }
            if !used.insert(key) {
                return Err(ProfileError::DuplicateKey(tap.key));
            }
            taps.push(Tap {
                key,
                x: tap.x,
                y: tap.y,
            });
        }
        Ok(Self {
            package: raw.package,
            toggle_key,
            taps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Profile, ProfileError, Tap};

    #[test]
    fn parses_a_profile() {
        let profile = Profile::parse(
            r#"
            package = "com.example.game"
            toggle_key = "F8"

            [[tap]]
            key = "R"
            x = 0.82
            y = 0.64

            [[tap]]
            key = "MouseRight"
            x = 0.5
            y = 0.5
            "#,
        )
        .unwrap();
        assert_eq!(profile.package, "com.example.game");
        assert_eq!(profile.toggle_key, Some(66));
        assert_eq!(
            profile.taps,
            vec![
                Tap {
                    key: 19,
                    x: 0.82,
                    y: 0.64
                },
                Tap {
                    key: 0x111,
                    x: 0.5,
                    y: 0.5
                }
            ]
        );
    }

    #[test]
    fn rejects_bad_profiles() {
        assert!(matches!(
            Profile::parse("package = \"\""),
            Err(ProfileError::EmptyPackage)
        ));
        assert!(matches!(
            Profile::parse("package = \"a\"\n[[tap]]\nkey = \"Banana\"\nx = 0.1\ny = 0.1"),
            Err(ProfileError::UnknownKey(_))
        ));
        assert!(matches!(
            Profile::parse("package = \"a\"\n[[tap]]\nkey = \"R\"\nx = 1.5\ny = 0.1"),
            Err(ProfileError::OutOfRange { .. })
        ));
        assert!(matches!(
            Profile::parse(
                "package = \"a\"\n[[tap]]\nkey = \"R\"\nx = 0.1\ny = 0.1\n[[tap]]\nkey = \"r\"\nx = 0.2\ny = 0.2"
            ),
            Err(ProfileError::DuplicateKey(_))
        ));
        assert!(matches!(
            Profile::parse(
                "package = \"a\"\ntoggle_key = \"R\"\n[[tap]]\nkey = \"R\"\nx = 0.1\ny = 0.1"
            ),
            Err(ProfileError::DuplicateKey(_))
        ));
        assert!(matches!(
            Profile::parse("package = \"a\"\nsurprise = 1"),
            Err(ProfileError::Syntax(_))
        ));
    }
}
