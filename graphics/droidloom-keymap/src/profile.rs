//! Per-game profile files.

use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;

use crate::keys::parse_key;

/// Most points one key may touch at once.
pub const MAX_TAP_POINTS: usize = 4;

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
    /// A tap has no usable point, or an aim has bad numbers.
    #[error(
        "tap needs x and y, or a list of points; aim anchor, region and cancel point must be inside the window"
    )]
    BadPoint,
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
    aim: Vec<RawAim>,
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
    x: Option<f32>,
    y: Option<f32>,
    points: Option<Vec<[f32; 2]>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAim {
    key: String,
    anchor: [f32; 2],
    region: [f32; 2],
    sensitivity: Option<f32>,
    cancel_key: Option<String>,
    cancel_at: Option<[f32; 2]>,
    stop_wheel: Option<bool>,
}

/// One key mapped to one or more fixed points in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Tap {
    /// Linux evdev key or button code.
    pub key: u16,
    /// Points touched together while the key is held, as fractions of the
    /// window: horizontal 0.0 (left) to 1.0 (right), vertical 0.0 (top) to
    /// 1.0 (bottom).
    pub points: Vec<(f32, f32)>,
}

/// Hold a key to put a touch down at a point and steer it with the mouse.
///
/// Used for observation views and for aimed skills: the touch goes down when
/// the key goes down, mouse movement drags it within a region, and it lifts
/// when the key is released.
#[derive(Clone, Debug, PartialEq)]
pub struct Aim {
    /// Linux evdev key or button code that holds the aim.
    pub key: u16,
    /// Where the touch goes down, as a fraction of the window.
    pub anchor: (f32, f32),
    /// How far it may be dragged from the anchor each way.
    pub region: (f32, f32),
    /// Touch movement per unit of mouse movement.
    pub sensitivity: f32,
    /// Optional key that abandons the aim, and the point the touch is moved
    /// to before it lifts (a cast-cancel button).
    pub cancel: Option<(u16, (f32, f32))>,
    /// Lift the joystick wheel touch when the aim starts.
    pub stop_wheel: bool,
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
    /// Hold-to-aim keys.
    pub aims: Vec<Aim>,
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
    /// [[aim]]                  # optional, repeatable: hold a key, steer a touch
    /// key = "Q"                # the key held to aim
    /// anchor = [0.9, 0.7]      # where the touch goes down (a skill button)
    /// region = [0.1, 0.15]     # how far the mouse may drag it
    /// cancel_key = "Space"     # optional: abandons the aim ...
    /// cancel_at = [0.8, 0.2]   # ... by moving the touch here and lifting it
    /// stop_wheel = false       # optional: lift the joystick wheel first
    ///
    /// [[tap]]
    /// key = "R"                # a key name, a number or "code:N"
    /// x = 0.82                 # 0.0 (left) to 1.0 (right)
    /// y = 0.64                 # 0.0 (top) to 1.0 (bottom)
    ///
    /// [[tap]]                  # one key touching several points at once
    /// key = "T"
    /// points = [[0.1, 0.2], [0.3, 0.4]]
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
        let wheel = raw
            .wheel
            .as_ref()
            .map(|wheel| parse_wheel(wheel, &mut used))
            .transpose()?;
        let look = raw.look.as_ref().map(parse_look).transpose()?;
        let aims = raw
            .aim
            .into_iter()
            .map(|aim| parse_aim(aim, &mut used))
            .collect::<Result<Vec<_>, _>>()?;
        let taps = raw
            .tap
            .into_iter()
            .map(|tap| parse_tap(tap, &mut used))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            package: raw.package,
            toggle_key,
            wheel,
            look,
            aims,
            taps,
        })
    }
}

fn in_range(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn positive_fraction(value: f32) -> bool {
    value.is_finite() && value > 0.0 && value <= 1.0
}

fn point_ok(point: [f32; 2]) -> bool {
    point.iter().copied().all(in_range)
}

/// Claim `name` as a new key or button, rejecting unknown and repeated ones.
fn claim_key(name: &str, used: &mut BTreeSet<u16>) -> Result<u16, ProfileError> {
    let code = parse_key(name).ok_or_else(|| ProfileError::UnknownKey(name.to_owned()))?;
    if used.insert(code) {
        Ok(code)
    } else {
        Err(ProfileError::DuplicateKey(name.to_owned()))
    }
}

fn parse_wheel(raw: &RawWheel, used: &mut BTreeSet<u16>) -> Result<Wheel, ProfileError> {
    if !point_ok(raw.center) || !raw.radius.iter().copied().all(positive_fraction) {
        return Err(ProfileError::BadWheel);
    }
    let mut keys = [0u16; 4];
    for (slot, name) in keys
        .iter_mut()
        .zip([&raw.up, &raw.down, &raw.left, &raw.right])
    {
        *slot = claim_key(name, used)?;
    }
    Ok(Wheel {
        center: (raw.center[0], raw.center[1]),
        radius: (raw.radius[0], raw.radius[1]),
        keys,
    })
}

fn parse_look(raw: &RawLook) -> Result<Look, ProfileError> {
    let sensitivity = raw.sensitivity.unwrap_or(1.0);
    if !point_ok(raw.anchor)
        || !raw.region.iter().copied().all(positive_fraction)
        || !sensitivity.is_finite()
        || sensitivity <= 0.0
    {
        return Err(ProfileError::BadLook);
    }
    Ok(Look {
        anchor: (raw.anchor[0], raw.anchor[1]),
        region: (raw.region[0], raw.region[1]),
        sensitivity,
    })
}

fn parse_aim(raw: RawAim, used: &mut BTreeSet<u16>) -> Result<Aim, ProfileError> {
    let key = claim_key(&raw.key, used)?;
    let sensitivity = raw.sensitivity.unwrap_or(1.0);
    if !point_ok(raw.anchor)
        || !raw.region.iter().copied().all(positive_fraction)
        || !sensitivity.is_finite()
        || sensitivity <= 0.0
    {
        return Err(ProfileError::BadPoint);
    }
    let cancel = match (raw.cancel_key, raw.cancel_at) {
        (Some(name), Some(at)) if point_ok(at) => Some((claim_key(&name, used)?, (at[0], at[1]))),
        (None, None) => None,
        _ => return Err(ProfileError::BadPoint),
    };
    Ok(Aim {
        key,
        anchor: (raw.anchor[0], raw.anchor[1]),
        region: (raw.region[0], raw.region[1]),
        sensitivity,
        cancel,
        stop_wheel: raw.stop_wheel.unwrap_or(false),
    })
}

fn parse_tap(raw: RawTap, used: &mut BTreeSet<u16>) -> Result<Tap, ProfileError> {
    let key = claim_key(&raw.key, used)?;
    let points: Vec<(f32, f32)> = match (raw.x, raw.y, raw.points) {
        (Some(x), Some(y), None) => vec![(x, y)],
        (None, None, Some(list)) if !list.is_empty() && list.len() <= MAX_TAP_POINTS => {
            list.into_iter().map(|point| (point[0], point[1])).collect()
        }
        _ => return Err(ProfileError::BadPoint),
    };
    if points.iter().any(|point| !point_ok([point.0, point.1])) {
        return Err(ProfileError::OutOfRange { key: raw.key });
    }
    Ok(Tap { key, points })
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
                    points: vec![(0.82, 0.64)]
                },
                Tap {
                    key: 0x111,
                    points: vec![(0.5, 0.5)]
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

    #[test]
    fn parses_multi_point_taps_and_aims() {
        let profile = Profile::parse(
            r#"
            package = "a"
            [[tap]]
            key = "T"
            points = [[0.1, 0.2], [0.3, 0.4]]
            [[aim]]
            key = "Q"
            anchor = [0.9, 0.7]
            region = [0.1, 0.15]
            cancel_key = "Space"
            cancel_at = [0.8, 0.2]
            stop_wheel = true
            [[aim]]
            key = "LeftAlt"
            anchor = [0.5, 0.5]
            region = [0.2, 0.2]
            sensitivity = 2.0
            "#,
        )
        .unwrap();
        assert_eq!(profile.taps[0].points, vec![(0.1, 0.2), (0.3, 0.4)]);
        assert_eq!(profile.aims.len(), 2);
        assert_eq!(profile.aims[0].cancel, Some((57, (0.8, 0.2))));
        assert!(profile.aims[0].stop_wheel);
        assert!(profile.aims[1].cancel.is_none());
    }

    #[test]
    fn rejects_bad_taps_and_aims() {
        let bad = [
            "[[tap]]\nkey = \"R\"\nx = 0.1",
            "[[tap]]\nkey = \"R\"\nx = 0.1\ny = 0.1\npoints = [[0.1, 0.1]]",
            "[[tap]]\nkey = \"R\"\npoints = []",
            "[[tap]]\nkey = \"R\"\npoints = [[0.1,0.1],[0.1,0.1],[0.1,0.1],[0.1,0.1],[0.1,0.1]]",
            "[[aim]]\nkey = \"Q\"\nanchor = [2.0, 0.5]\nregion = [0.1, 0.1]",
            "[[aim]]\nkey = \"Q\"\nanchor = [0.5, 0.5]\nregion = [0.0, 0.1]",
            "[[aim]]\nkey = \"Q\"\nanchor = [0.5, 0.5]\nregion = [0.1, 0.1]\ncancel_key = \"Space\"",
        ];
        for text in bad {
            assert!(
                matches!(
                    Profile::parse(&format!("package = \"a\"\n{text}")),
                    Err(ProfileError::BadPoint)
                ),
                "{text}"
            );
        }
        assert!(matches!(
            Profile::parse(
                "package = \"a\"\n[[tap]]\nkey = \"R\"\nx = 0.1\ny = 0.1\n[[aim]]\nkey = \"R\"\nanchor = [0.5, 0.5]\nregion = [0.1, 0.1]"
            ),
            Err(ProfileError::DuplicateKey(_))
        ));
    }

    #[test]
    fn the_shipped_example_profile_is_valid() {
        let profile = Profile::parse(include_str!("../../../profiles/example.toml")).unwrap();
        assert_eq!(profile.package, "com.example.game");
        assert!(profile.wheel.is_some());
        assert!(profile.look.is_some());
        assert_eq!(profile.aims.len(), 1);
        assert_eq!(profile.taps.len(), 3);
    }
}
