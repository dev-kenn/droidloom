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
    /// The look anchor, region or sensitivity is not usable.
    #[error(
        "look anchor must be inside the window, region between 0.0 and 1.0, sensitivity above 0"
    )]
    BadLook,
    /// The wheel centre or radius is not usable.
    #[error("wheel centre must be inside the window and radius between 0.0 and 1.0")]
    BadWheel,
    /// The same key or button is mapped twice, or is also the toggle key.
    #[error("key or button is used twice: {0}")]
    DuplicateKey(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    package: String,
    toggle_key: Option<String>,
    wheel: Option<RawWheel>,
    look: Option<RawLook>,
    #[serde(default)]
    tap: Vec<RawTap>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLook {
    anchor: [f32; 2],
    region: [f32; 2],
    sensitivity: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWheel {
    center: [f32; 2],
    radius: [f32; 2],
    up: String,
    down: String,
    left: String,
    right: String,
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

/// Four keys that steer one virtual joystick touch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    /// Where the joystick touch rests, as a fraction of the window.
    pub center: (f32, f32),
    /// How far a full push moves the touch, as a fraction of width and height.
    pub radius: (f32, f32),
    /// Evdev codes for up, down, left and right.
    pub keys: [u16; 4],
}

impl Wheel {
    /// Index of `code` in `keys`: 0 up, 1 down, 2 left, 3 right.
    #[must_use]
    pub fn direction(&self, code: u16) -> Option<usize> {
        self.keys.iter().position(|key| *key == code)
    }
}

/// Mouse-look: mouse movement drags one touch around a region of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Where the look touch starts, as a fraction of the window.
    pub anchor: (f32, f32),
    /// How far the touch may travel from the anchor each way, as a fraction
    /// of the window width and height. Past that it lifts and starts again
    /// at the anchor.
    pub region: (f32, f32),
    /// Touch movement per unit of mouse movement. 1.0 moves the touch by the
    /// same fraction of the window as the mouse moved.
    pub sensitivity: f32,
}

/// A validated keymap for one Android package.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Android package the profile applies to.
    pub package: String,
    /// Optional key that switches the mapper on and off.
    pub toggle_key: Option<u16>,
    /// Optional joystick wheel.
    pub wheel: Option<Wheel>,
    /// Optional mouse-look.
    pub look: Option<Look>,
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
    /// [wheel]                  # optional: four keys steer one touch
    /// center = [0.18, 0.72]    # where the touch rests (fraction of the window)
    /// radius = [0.08, 0.14]    # full push, as a fraction of width and height
    /// up = "W"
    /// down = "S"
    /// left = "A"
    /// right = "D"
    ///
    /// [look]                   # optional: mouse movement drags one touch
    /// anchor = [0.65, 0.5]     # where the look touch starts
    /// region = [0.2, 0.3]      # how far it may travel from the anchor
    /// sensitivity = 1.0        # optional, default 1.0
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
        let wheel = match raw.wheel {
            Some(raw_wheel) => {
                let in_range = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
                let radius_ok = |value: f32| value.is_finite() && value > 0.0 && value <= 1.0;
                if !raw_wheel.center.iter().copied().all(in_range)
                    || !raw_wheel.radius.iter().copied().all(radius_ok)
                {
                    return Err(ProfileError::BadWheel);
                }
                let mut keys = [0u16; 4];
                for (slot, name) in keys.iter_mut().zip([
                    &raw_wheel.up,
                    &raw_wheel.down,
                    &raw_wheel.left,
                    &raw_wheel.right,
                ]) {
                    let code = parse_key(name).ok_or(ProfileError::UnknownKey(name.clone()))?;
                    if !used.insert(code) {
                        return Err(ProfileError::DuplicateKey(name.clone()));
                    }
                    *slot = code;
                }
                Some(Wheel {
                    center: (raw_wheel.center[0], raw_wheel.center[1]),
                    radius: (raw_wheel.radius[0], raw_wheel.radius[1]),
                    keys,
                })
            }
            None => None,
        };
        let look = match raw.look {
            Some(raw_look) => {
                let in_range = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
                let sensitivity = raw_look.sensitivity.unwrap_or(1.0);
                if !raw_look.anchor.iter().copied().all(in_range)
                    || !raw_look
                        .region
                        .iter()
                        .all(|value| value.is_finite() && *value > 0.0 && *value <= 1.0)
                    || !sensitivity.is_finite()
                    || sensitivity <= 0.0
                {
                    return Err(ProfileError::BadLook);
                }
                Some(Look {
                    anchor: (raw_look.anchor[0], raw_look.anchor[1]),
                    region: (raw_look.region[0], raw_look.region[1]),
                    sensitivity,
                })
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
            wheel,
            look,
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

    #[test]
    fn parses_and_validates_a_wheel() {
        let profile = Profile::parse(
            r#"
            package = "a"
            [wheel]
            center = [0.2, 0.7]
            radius = [0.1, 0.15]
            up = "W"
            down = "S"
            left = "A"
            right = "D"
            "#,
        )
        .unwrap();
        let wheel = profile.wheel.unwrap();
        assert_eq!(wheel.center, (0.2, 0.7));
        assert_eq!(wheel.keys, [17, 31, 30, 32]);
        assert_eq!(wheel.direction(31), Some(1));
        assert_eq!(wheel.direction(18), None);
        let base =
            "package = \"a\"\n[wheel]\nup = \"W\"\ndown = \"S\"\nleft = \"A\"\nright = \"D\"\n";
        assert!(matches!(
            Profile::parse(&format!("{base}center = [1.5, 0.5]\nradius = [0.1, 0.1]")),
            Err(ProfileError::BadWheel)
        ));
        assert!(matches!(
            Profile::parse(&format!("{base}center = [0.5, 0.5]\nradius = [0.0, 0.1]")),
            Err(ProfileError::BadWheel)
        ));
        assert!(matches!(
            Profile::parse(&format!(
                "{base}center = [0.5, 0.5]\nradius = [0.1, 0.1]\n[[tap]]\nkey = \"W\"\nx = 0.1\ny = 0.1"
            )),
            Err(ProfileError::DuplicateKey(_))
        ));
    }

    #[test]
    fn parses_and_validates_a_look() {
        let profile = Profile::parse(
            "package = \"a\"\n[look]\nanchor = [0.6, 0.5]\nregion = [0.2, 0.3]\nsensitivity = 2.0",
        )
        .unwrap();
        let look = profile.look.unwrap();
        assert_eq!(look.anchor, (0.6, 0.5));
        assert!((look.sensitivity - 2.0).abs() < f32::EPSILON);
        let default =
            Profile::parse("package = \"a\"\n[look]\nanchor = [0.6, 0.5]\nregion = [0.2, 0.3]")
                .unwrap();
        assert!((default.look.unwrap().sensitivity - 1.0).abs() < f32::EPSILON);
        for bad in [
            "anchor = [1.2, 0.5]\nregion = [0.2, 0.3]",
            "anchor = [0.5, 0.5]\nregion = [0.0, 0.3]",
            "anchor = [0.5, 0.5]\nregion = [0.2, 0.3]\nsensitivity = 0.0",
        ] {
            assert!(matches!(
                Profile::parse(&format!("package = \"a\"\n[look]\n{bad}")),
                Err(ProfileError::BadLook)
            ));
        }
    }
}
