//! The key to touch state machine.

use std::collections::BTreeMap;

use crate::profile::{Look, Profile, Wheel};

/// First pointer identity used for mapped contacts. Real touch and mouse
/// contacts use small identities and `u32::MAX`, so these never collide.
pub const SYNTHETIC_POINTER_BASE: u32 = 0x8000_0000;

/// Most mapped contacts held at once, matching the original Game Space limit.
pub const MAX_CONTACTS: usize = SLOTS as usize;

const SLOTS: u32 = 10;

/// Share of a full push each axis gets when two directions are held, so the
/// diagonal has the same reach as a straight push.
const DIAGONAL: f32 = 0.707;

/// The joystick contact driven by the wheel keys.
#[derive(Debug, Default)]
struct WheelState {
    /// Up, down, left, right held.
    held: [bool; 4],
    /// Slot the joystick touch occupies while it is down.
    slot: Option<u32>,
    /// Where the touch currently is.
    at: (f32, f32),
}

/// The touch dragged around by mouse movement.
#[derive(Debug, Default)]
struct LookState {
    /// The host has the pointer locked, so motion can steer the touch.
    active: bool,
    /// Slot the look touch occupies while it is down.
    slot: Option<u32>,
    /// Where the touch currently is.
    at: (f32, f32),
}

/// What a key or button did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputState {
    /// The key or button went down.
    Pressed,
    /// The key or button came up.
    Released,
    /// The key is auto-repeating while held.
    Repeat,
}

/// Touch phase of a mapped contact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// A contact starts.
    Down,
    /// A held contact moves.
    Move,
    /// A contact ends normally.
    Up,
    /// A contact is abandoned, for example on focus loss.
    Cancel,
}

/// One touch record for the presenter to send to Android.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchOut {
    /// What happens to the contact.
    pub phase: Phase,
    /// Pointer identity, at least [`SYNTHETIC_POINTER_BASE`].
    pub pointer_id: u32,
    /// Horizontal position, 0.0 to 1.0 of the window width.
    pub x: f32,
    /// Vertical position, 0.0 to 1.0 of the window height.
    pub y: f32,
}

/// The result of feeding one key or button to the mapper.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// The mapper handled the key: do not also send it to Android.
    pub consumed: bool,
    /// Touch records to send, in order.
    pub touches: Vec<TouchOut>,
}

/// Turns keyboard and mouse button events into touch contacts.
#[derive(Debug)]
pub struct Mapper {
    profile: Profile,
    enabled: bool,
    /// Held key code to the slot (0 to `MAX_CONTACTS` - 1) it occupies.
    held: BTreeMap<u16, u32>,
    wheel: WheelState,
    look: LookState,
}

impl Mapper {
    /// Create a mapper for a profile. It starts enabled.
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            enabled: true,
            held: BTreeMap::new(),
            wheel: WheelState::default(),
            look: LookState::default(),
        }
    }

    /// The profile in use.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Whether mapping is on.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// True when no mapped contact is held.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.held.is_empty() && self.wheel.slot.is_none() && self.look.slot.is_none()
    }

    /// Turn mapping on or off. Turning it off ends every held contact.
    pub fn set_enabled(&mut self, enabled: bool) -> Vec<TouchOut> {
        if self.enabled == enabled {
            return Vec::new();
        }
        self.enabled = enabled;
        if enabled {
            Vec::new()
        } else {
            self.release_all(Phase::Cancel)
        }
    }

    /// End every held contact, for focus loss or window changes.
    pub fn release_all(&mut self, phase: Phase) -> Vec<TouchOut> {
        let held = std::mem::take(&mut self.held);
        let mut ended: Vec<TouchOut> = held
            .into_iter()
            .filter_map(|(key, slot)| {
                let tap = self.profile.taps.iter().find(|tap| tap.key == key)?;
                Some(TouchOut {
                    phase,
                    pointer_id: SYNTHETIC_POINTER_BASE + slot,
                    x: tap.x,
                    y: tap.y,
                })
            })
            .collect();
        if let Some(slot) = self.wheel.slot.take() {
            ended.push(TouchOut {
                phase,
                pointer_id: SYNTHETIC_POINTER_BASE + slot,
                x: self.wheel.at.0,
                y: self.wheel.at.1,
            });
        }
        self.wheel.held = [false; 4];
        if let Some(slot) = self.look.slot.take() {
            ended.push(TouchOut {
                phase,
                pointer_id: SYNTHETIC_POINTER_BASE + slot,
                x: self.look.at.0,
                y: self.look.at.1,
            });
        }
        ended
    }

    /// True when the profile has mouse-look and mapping is on, so the host
    /// should lock the pointer and send relative motion.
    #[must_use]
    pub fn look_wanted(&self) -> bool {
        self.enabled && self.profile.look.is_some()
    }

    /// True while the pointer is locked and mouse-look steers the touch.
    #[must_use]
    pub fn look_active(&self) -> bool {
        self.look.active
    }

    /// The host locked or unlocked the pointer. Unlocking lifts the look touch.
    pub fn set_look_active(&mut self, active: bool) -> Vec<TouchOut> {
        self.look.active = active;
        let mut touches = Vec::new();
        if !active && let Some(slot) = self.look.slot.take() {
            touches.push(self.look_touch(Phase::Up, slot));
        }
        touches
    }

    /// Relative mouse movement as fractions of the window width and height.
    pub fn look_motion(&mut self, dx: f32, dy: f32) -> Vec<TouchOut> {
        let Some(look) = self.profile.look else {
            return Vec::new();
        };
        if !self.enabled || !self.look.active || !dx.is_finite() || !dy.is_finite() {
            return Vec::new();
        }
        let mut touches = Vec::new();
        if self.look.slot.is_none() {
            let Some(slot) = self.free_slot() else {
                return touches;
            };
            self.look.slot = Some(slot);
            self.look.at = look.anchor;
            touches.push(self.look_touch(Phase::Down, slot));
        }
        let Some(slot) = self.look.slot else {
            return touches;
        };
        let next = (
            self.look.at.0 + dx * look.sensitivity,
            self.look.at.1 + dy * look.sensitivity,
        );
        let inside = |value: f32, anchor: f32, region: f32| (value - anchor).abs() <= region;
        let next = if inside(next.0, look.anchor.0, look.region.0)
            && inside(next.1, look.anchor.1, look.region.1)
        {
            next
        } else {
            touches.push(self.look_touch(Phase::Up, slot));
            self.look.at = look.anchor;
            let slot_down = self.look_touch(Phase::Down, slot);
            touches.push(slot_down);
            Self::limit_to_region(
                &look,
                (
                    look.anchor.0 + dx * look.sensitivity,
                    look.anchor.1 + dy * look.sensitivity,
                ),
            )
        };
        if self.look.at != next {
            self.look.at = next;
            touches.push(self.look_touch(Phase::Move, slot));
        }
        touches
    }

    fn limit_to_region(look: &Look, point: (f32, f32)) -> (f32, f32) {
        (
            point
                .0
                .clamp(look.anchor.0 - look.region.0, look.anchor.0 + look.region.0)
                .clamp(0.0, 1.0),
            point
                .1
                .clamp(look.anchor.1 - look.region.1, look.anchor.1 + look.region.1)
                .clamp(0.0, 1.0),
        )
    }

    fn look_touch(&self, phase: Phase, slot: u32) -> TouchOut {
        TouchOut {
            phase,
            pointer_id: SYNTHETIC_POINTER_BASE + slot,
            x: self.look.at.0,
            y: self.look.at.1,
        }
    }

    /// Feed one key or button event.
    pub fn input(&mut self, code: u16, state: InputState) -> Outcome {
        if self.profile.toggle_key == Some(code) {
            if state == InputState::Pressed {
                let enabled = !self.enabled;
                return Outcome {
                    consumed: true,
                    touches: self.set_enabled(enabled),
                };
            }
            return Outcome {
                consumed: true,
                touches: Vec::new(),
            };
        }
        if !self.enabled {
            return Outcome::default();
        }
        if let Some(wheel) = self.profile.wheel
            && let Some(direction) = wheel.direction(code)
        {
            return self.wheel_input(&wheel, direction, state);
        }
        let Some(tap) = self
            .profile
            .taps
            .iter()
            .find(|tap| tap.key == code)
            .copied()
        else {
            return Outcome::default();
        };
        let mut touches = Vec::new();
        match state {
            InputState::Pressed => {
                if !self.held.contains_key(&code)
                    && let Some(slot) = self.free_slot()
                {
                    self.held.insert(code, slot);
                    touches.push(TouchOut {
                        phase: Phase::Down,
                        pointer_id: SYNTHETIC_POINTER_BASE + slot,
                        x: tap.x,
                        y: tap.y,
                    });
                }
            }
            InputState::Released => {
                if let Some(slot) = self.held.remove(&code) {
                    touches.push(TouchOut {
                        phase: Phase::Up,
                        pointer_id: SYNTHETIC_POINTER_BASE + slot,
                        x: tap.x,
                        y: tap.y,
                    });
                }
            }
            InputState::Repeat => {}
        }
        Outcome {
            consumed: true,
            touches,
        }
    }

    fn free_slot(&self) -> Option<u32> {
        (0..SLOTS).find(|slot| {
            !self.held.values().any(|held| held == slot)
                && self.wheel.slot != Some(*slot)
                && self.look.slot != Some(*slot)
        })
    }

    /// Where the joystick touch should be for the directions now held.
    fn wheel_target(wheel: &Wheel, held: [bool; 4]) -> (f32, f32) {
        let axis = |positive: bool, negative: bool| match (positive, negative) {
            (true, false) => 1.0_f32,
            (false, true) => -1.0,
            _ => 0.0,
        };
        let mut dx = axis(held[3], held[2]);
        let mut dy = axis(held[1], held[0]);
        if dx != 0.0 && dy != 0.0 {
            dx *= DIAGONAL;
            dy *= DIAGONAL;
        }
        (
            (wheel.center.0 + dx * wheel.radius.0).clamp(0.0, 1.0),
            (wheel.center.1 + dy * wheel.radius.1).clamp(0.0, 1.0),
        )
    }

    fn wheel_input(&mut self, wheel: &Wheel, direction: usize, state: InputState) -> Outcome {
        let mut touches = Vec::new();
        match state {
            InputState::Repeat => {}
            InputState::Pressed => {
                if !self.wheel.held[direction] {
                    self.wheel.held[direction] = true;
                    let target = Self::wheel_target(wheel, self.wheel.held);
                    if self.wheel.slot.is_none()
                        && let Some(slot) = self.free_slot()
                    {
                        self.wheel.slot = Some(slot);
                        self.wheel.at = wheel.center;
                        touches.push(self.wheel_touch_at_slot(Phase::Down, slot));
                    }
                    self.wheel_move(target, &mut touches);
                }
            }
            InputState::Released => {
                if self.wheel.held[direction] {
                    self.wheel.held[direction] = false;
                    if self.wheel.held.iter().any(|held| *held) {
                        let target = Self::wheel_target(wheel, self.wheel.held);
                        self.wheel_move(target, &mut touches);
                    } else if let Some(slot) = self.wheel.slot {
                        self.wheel_move(wheel.center, &mut touches);
                        touches.push(self.wheel_touch_at_slot(Phase::Up, slot));
                        self.wheel.slot = None;
                    }
                }
            }
        }
        Outcome {
            consumed: true,
            touches,
        }
    }

    fn wheel_touch_at_slot(&self, phase: Phase, slot: u32) -> TouchOut {
        TouchOut {
            phase,
            pointer_id: SYNTHETIC_POINTER_BASE + slot,
            x: self.wheel.at.0,
            y: self.wheel.at.1,
        }
    }

    /// Move the joystick touch to `target` if it is down and not already there.
    fn wheel_move(&mut self, target: (f32, f32), touches: &mut Vec<TouchOut>) {
        let Some(slot) = self.wheel.slot else {
            return;
        };
        if self.wheel.at != target {
            self.wheel.at = target;
            touches.push(self.wheel_touch_at_slot(Phase::Move, slot));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::{InputState, Mapper, Phase, SYNTHETIC_POINTER_BASE};
    use crate::Profile;

    const R: u16 = 19;
    const E: u16 = 18;
    const F8: u16 = 66;

    fn mapper() -> Mapper {
        Mapper::new(
            Profile::parse(
                r#"
                package = "com.example.game"
                toggle_key = "F8"
                [[tap]]
                key = "R"
                x = 0.8
                y = 0.6
                [[tap]]
                key = "E"
                x = 0.2
                y = 0.4
                "#,
            )
            .unwrap(),
        )
    }

    #[test]
    fn press_and_release_make_one_tap_at_the_profile_point() {
        let mut mapper = mapper();
        let down = mapper.input(R, InputState::Pressed);
        assert!(down.consumed);
        assert_eq!(down.touches.len(), 1);
        assert_eq!(down.touches[0].phase, Phase::Down);
        assert_eq!(down.touches[0].pointer_id, SYNTHETIC_POINTER_BASE);
        assert_eq!((down.touches[0].x, down.touches[0].y), (0.8, 0.6));
        let up = mapper.input(R, InputState::Released);
        assert_eq!(up.touches.len(), 1);
        assert_eq!(up.touches[0].phase, Phase::Up);
        assert_eq!(up.touches[0].pointer_id, SYNTHETIC_POINTER_BASE);
        assert!(mapper.is_idle());
    }

    #[test]
    fn two_keys_hold_two_contacts_and_reuse_the_lowest_slot() {
        let mut mapper = mapper();
        let first = mapper.input(R, InputState::Pressed).touches[0];
        let second = mapper.input(E, InputState::Pressed).touches[0];
        assert_eq!(first.pointer_id, SYNTHETIC_POINTER_BASE);
        assert_eq!(second.pointer_id, SYNTHETIC_POINTER_BASE + 1);
        mapper.input(R, InputState::Released);
        let again = mapper.input(R, InputState::Pressed).touches[0];
        assert_eq!(again.pointer_id, SYNTHETIC_POINTER_BASE);
    }

    #[test]
    fn repeat_and_duplicate_presses_make_no_extra_touches() {
        let mut mapper = mapper();
        mapper.input(R, InputState::Pressed);
        assert!(mapper.input(R, InputState::Repeat).touches.is_empty());
        assert!(mapper.input(R, InputState::Pressed).touches.is_empty());
        assert!(mapper.input(R, InputState::Repeat).consumed);
    }

    #[test]
    fn unmapped_keys_pass_through() {
        let mut mapper = mapper();
        let outcome = mapper.input(30, InputState::Pressed);
        assert!(!outcome.consumed);
        assert!(outcome.touches.is_empty());
    }

    #[test]
    fn release_without_press_is_consumed_silently() {
        let mut mapper = mapper();
        let outcome = mapper.input(R, InputState::Released);
        assert!(outcome.consumed);
        assert!(outcome.touches.is_empty());
    }

    #[test]
    fn toggle_key_cancels_held_contacts_and_passes_keys_through_while_off() {
        let mut mapper = mapper();
        mapper.input(R, InputState::Pressed);
        let off = mapper.input(F8, InputState::Pressed);
        assert!(off.consumed);
        assert_eq!(off.touches.len(), 1);
        assert_eq!(off.touches[0].phase, Phase::Cancel);
        assert!(!mapper.enabled());
        assert!(!mapper.input(R, InputState::Pressed).consumed);
        mapper.input(F8, InputState::Released);
        mapper.input(F8, InputState::Pressed);
        assert!(mapper.enabled());
        assert!(mapper.input(R, InputState::Pressed).consumed);
    }

    #[test]
    fn release_all_ends_every_contact_once() {
        let mut mapper = mapper();
        mapper.input(R, InputState::Pressed);
        mapper.input(E, InputState::Pressed);
        let ended = mapper.release_all(Phase::Cancel);
        assert_eq!(ended.len(), 2);
        assert!(ended.iter().all(|touch| touch.phase == Phase::Cancel));
        assert!(mapper.release_all(Phase::Cancel).is_empty());
        assert!(mapper.is_idle());
    }

    #[test]
    fn contact_limit_consumes_the_key_but_sends_nothing() {
        let mut keys = String::from("package = \"a\"\n");
        for code in 0..12 {
            writeln!(
                keys,
                "[[tap]]\nkey = \"code:{}\"\nx = 0.5\ny = 0.5",
                200 + code
            )
            .unwrap();
        }
        let mut mapper = Mapper::new(Profile::parse(&keys).unwrap());
        for code in 0..10u16 {
            assert_eq!(
                mapper.input(200 + code, InputState::Pressed).touches.len(),
                1
            );
        }
        let over = mapper.input(210, InputState::Pressed);
        assert!(over.consumed);
        assert!(over.touches.is_empty());
    }

    fn wheel_mapper() -> Mapper {
        Mapper::new(
            Profile::parse(
                r#"
                package = "com.example.game"
                [wheel]
                center = [0.2, 0.7]
                radius = [0.1, 0.2]
                up = "W"
                down = "S"
                left = "A"
                right = "D"
                [[tap]]
                key = "R"
                x = 0.8
                y = 0.6
                "#,
            )
            .unwrap(),
        )
    }

    const W: u16 = 17;
    const A: u16 = 30;
    const S: u16 = 31;
    const D: u16 = 32;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn first_wheel_key_puts_the_touch_down_at_the_centre_then_pushes_it() {
        let mut mapper = wheel_mapper();
        let touches = mapper.input(W, InputState::Pressed).touches;
        assert_eq!(touches.len(), 2);
        assert_eq!(touches[0].phase, Phase::Down);
        assert_eq!((touches[0].x, touches[0].y), (0.2, 0.7));
        assert_eq!(touches[1].phase, Phase::Move);
        assert_eq!(touches[0].pointer_id, touches[1].pointer_id);
        assert!(near(touches[1].x, 0.2));
        assert!(near(touches[1].y, 0.5));
    }

    #[test]
    fn diagonals_use_the_reduced_reach_and_opposites_cancel() {
        let mut mapper = wheel_mapper();
        mapper.input(W, InputState::Pressed);
        let diagonal = mapper.input(D, InputState::Pressed).touches;
        assert_eq!(diagonal.len(), 1);
        assert_eq!(diagonal[0].phase, Phase::Move);
        assert!(near(diagonal[0].x, 0.2 + 0.1 * 0.707));
        assert!(near(diagonal[0].y, 0.7 - 0.2 * 0.707));
        let cancelled = mapper.input(A, InputState::Pressed).touches;
        assert_eq!(cancelled.len(), 1);
        assert!(near(cancelled[0].x, 0.2));
        assert!(near(cancelled[0].y, 0.5));
    }

    #[test]
    fn releasing_every_wheel_key_returns_to_centre_and_lifts() {
        let mut mapper = wheel_mapper();
        mapper.input(W, InputState::Pressed);
        let partial = mapper.input(W, InputState::Released);
        assert!(partial.consumed);
        assert_eq!(partial.touches.len(), 2);
        assert_eq!(partial.touches[0].phase, Phase::Move);
        assert!(near(partial.touches[0].y, 0.7));
        assert_eq!(partial.touches[1].phase, Phase::Up);
        assert!(mapper.is_idle());
    }

    #[test]
    fn wheel_and_taps_use_different_contacts() {
        let mut mapper = wheel_mapper();
        let wheel = mapper.input(W, InputState::Pressed).touches[0].pointer_id;
        let tap = mapper.input(R, InputState::Pressed).touches[0].pointer_id;
        assert_ne!(wheel, tap);
        assert_eq!(wheel, SYNTHETIC_POINTER_BASE);
    }

    #[test]
    fn repeats_and_stray_releases_on_the_wheel_do_nothing() {
        let mut mapper = wheel_mapper();
        assert!(mapper.input(W, InputState::Released).touches.is_empty());
        mapper.input(W, InputState::Pressed);
        assert!(mapper.input(W, InputState::Repeat).touches.is_empty());
        assert!(mapper.input(W, InputState::Pressed).touches.is_empty());
        assert!(mapper.input(S, InputState::Released).touches.is_empty());
    }

    #[test]
    fn release_all_ends_the_wheel_contact_too() {
        let mut mapper = wheel_mapper();
        mapper.input(W, InputState::Pressed);
        mapper.input(R, InputState::Pressed);
        let ended = mapper.release_all(Phase::Cancel);
        assert_eq!(ended.len(), 2);
        assert!(mapper.is_idle());
        assert!(mapper.input(D, InputState::Pressed).consumed);
    }

    fn look_mapper() -> Mapper {
        Mapper::new(
            Profile::parse(
                r#"
                package = "com.example.game"
                [look]
                anchor = [0.6, 0.5]
                region = [0.1, 0.2]
                [[tap]]
                key = "MouseLeft"
                x = 0.9
                y = 0.8
                "#,
            )
            .unwrap(),
        )
    }

    #[test]
    fn look_needs_a_locked_pointer() {
        let mut mapper = look_mapper();
        assert!(mapper.look_wanted());
        assert!(!mapper.look_active());
        assert!(mapper.look_motion(0.01, 0.0).is_empty());
        mapper.set_look_active(true);
        assert!(mapper.look_active());
        assert!(!mapper.look_motion(0.01, 0.0).is_empty());
    }

    #[test]
    fn first_motion_puts_the_touch_down_at_the_anchor_then_drags_it() {
        let mut mapper = look_mapper();
        mapper.set_look_active(true);
        let touches = mapper.look_motion(0.02, -0.05);
        assert_eq!(touches.len(), 2);
        assert_eq!(touches[0].phase, Phase::Down);
        assert_eq!((touches[0].x, touches[0].y), (0.6, 0.5));
        assert_eq!(touches[1].phase, Phase::Move);
        assert!(near(touches[1].x, 0.62));
        assert!(near(touches[1].y, 0.45));
        let more = mapper.look_motion(0.01, 0.0);
        assert_eq!(more.len(), 1);
        assert!(near(more[0].x, 0.63));
    }

    #[test]
    fn leaving_the_region_lifts_and_restarts_at_the_anchor() {
        let mut mapper = look_mapper();
        mapper.set_look_active(true);
        mapper.look_motion(0.09, 0.0);
        let touches = mapper.look_motion(0.05, 0.0);
        let phases: Vec<_> = touches.iter().map(|touch| touch.phase).collect();
        assert_eq!(phases, vec![Phase::Up, Phase::Down, Phase::Move]);
        assert!(near(touches[0].x, 0.69));
        assert_eq!((touches[1].x, touches[1].y), (0.6, 0.5));
        assert!(near(touches[2].x, 0.65));
        assert_eq!(touches[0].pointer_id, touches[1].pointer_id);
    }

    #[test]
    fn unlocking_lifts_the_look_touch_and_a_huge_motion_stays_in_the_region() {
        let mut mapper = look_mapper();
        mapper.set_look_active(true);
        let jump = mapper.look_motion(5.0, 0.0);
        let last = jump.last().unwrap();
        assert!(near(last.x, 0.7));
        let lifted = mapper.set_look_active(false);
        assert_eq!(lifted.len(), 1);
        assert_eq!(lifted[0].phase, Phase::Up);
        assert!(mapper.is_idle());
        assert!(mapper.look_motion(0.01, 0.0).is_empty());
    }

    #[test]
    fn toggling_off_drops_the_look_and_stops_wanting_the_lock() {
        let mut mapper = look_mapper();
        mapper.set_look_active(true);
        mapper.look_motion(0.01, 0.0);
        assert!(
            mapper
                .set_enabled(false)
                .iter()
                .all(|touch| touch.phase == Phase::Cancel)
        );
        assert!(!mapper.look_wanted());
        assert!(mapper.look_motion(0.01, 0.0).is_empty());
        assert!(mapper.is_idle());
    }

    #[test]
    fn mouse_left_can_be_a_tap_while_look_is_active() {
        let mut mapper = look_mapper();
        mapper.set_look_active(true);
        let down = mapper.input(0x110, InputState::Pressed);
        assert!(down.consumed);
        assert_eq!((down.touches[0].x, down.touches[0].y), (0.9, 0.8));
    }
}
