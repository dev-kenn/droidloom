//! Keyboard and mouse button to touch mapping for the focused game window.
//!
//! The mapping itself lives in `droidloom-keymap`. This module loads the
//! profile for the focused task's package and sends the touches it produces
//! through the same input path as real touches. A profile is a TOML file at
//! `$XDG_CONFIG_HOME/droidloom/keymaps/<package>.toml` (or under
//! `~/.config`). It is read each time a window gains focus, so edits take
//! effect the next time you click back into the game.

use std::fs;
use std::io;
use std::path::PathBuf;

use droidloom_denial_protocol::{InputEvent, TaskObjectId, TouchAction};
use droidloom_keymap::{InputState, Mapper, Phase, Profile, TouchOut};
use smithay_client_toolkit::reexports::client::protocol::wl_pointer;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::ZwpConfinedPointerV1;
use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::ZwpLockedPointerV1;
use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime;
use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1;
use smithay_client_toolkit::seat::pointer_constraints::PointerConstraintsHandler;
use smithay_client_toolkit::seat::relative_pointer::{RelativeMotionEvent, RelativePointerHandler};

use crate::{App, BOOTSTRAP_PACKAGE};

/// The pointer lock and relative pointer held while mouse-look is on.
pub(crate) struct PointerLock {
    locked: ZwpLockedPointerV1,
    relative: ZwpRelativePointerV1,
}

/// Path of the profile for an Android package, if the name is safe to use.
fn profile_path(package: &str) -> Option<PathBuf> {
    let safe = !package.is_empty()
        && package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !package.contains("..");
    if !safe {
        return None;
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(
        base.join("droidloom")
            .join("keymaps")
            .join(format!("{package}.toml")),
    )
}

/// Read the profile for `package`, or None when there is none or it is invalid.
fn load_mapper(package: &str) -> Option<Mapper> {
    let path = profile_path(package)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            eprintln!("Droidloom keymap: cannot read {}: {error}", path.display());
            return None;
        }
    };
    match Profile::parse(&text) {
        Ok(profile) if profile.package == package => Some(Mapper::new(profile)),
        Ok(profile) => {
            eprintln!(
                "Droidloom keymap: {} is for {}, not {package}",
                path.display(),
                profile.package
            );
            None
        }
        Err(error) => {
            eprintln!("Droidloom keymap: {}: {error}", path.display());
            None
        }
    }
}

impl App {
    /// Use the profile of the newly focused task, if it has one.
    pub(crate) fn keymap_focus(&mut self, object: Option<TaskObjectId>) {
        self.keymap = object
            .and_then(|object| self.tasks.get(&object))
            .filter(|task| task.package != BOOTSTRAP_PACKAGE)
            .and_then(|task| load_mapper(&task.package));
    }

    /// End every mapped contact in the task that is losing focus.
    pub(crate) fn keymap_release(&mut self, object: TaskObjectId) {
        let Some(mapper) = self.keymap.as_mut() else {
            return;
        };
        let ended = mapper.release_all(Phase::Cancel);
        self.keymap_send(object, &ended);
    }

    /// True while the pointer is locked and mouse-look is steering a touch.
    pub(crate) fn keymap_look_active(&self) -> bool {
        self.keymap.as_ref().is_some_and(Mapper::look_active)
    }

    /// Lock the pointer when the focused game wants mouse-look, and release it
    /// when it no longer does (another window, or mapping toggled off).
    pub(crate) fn keymap_sync_lock(&mut self) {
        let wanted = self.keymap.as_ref().is_some_and(Mapper::look_wanted);
        if wanted && self.pointer_lock.is_none() {
            self.take_pointer_lock();
        } else if !wanted && self.pointer_lock.is_some() {
            self.drop_pointer_lock();
        }
    }

    fn take_pointer_lock(&mut self) {
        let (Some(object), Some(pointer)) = (self.focused, self.pointer.clone()) else {
            return;
        };
        let Some(surface) = self
            .tasks
            .get(&object)
            .and_then(|task| task.surface())
            .cloned()
        else {
            return;
        };
        let locked = match self.pointer_constraints.lock_pointer(
            &surface,
            &pointer,
            None,
            Lifetime::Persistent,
            &self.qh,
        ) {
            Ok(locked) => locked,
            Err(error) => {
                eprintln!("Droidloom keymap: the compositor cannot lock the pointer: {error}");
                return;
            }
        };
        match self
            .relative_pointer
            .get_relative_pointer(&pointer, &self.qh)
        {
            Ok(relative) => self.pointer_lock = Some(PointerLock { locked, relative }),
            Err(error) => {
                eprintln!("Droidloom keymap: the compositor has no relative pointer: {error}");
                locked.destroy();
            }
        }
    }

    /// Release the pointer lock and lift the look touch.
    pub(crate) fn drop_pointer_lock(&mut self) {
        if let Some(lock) = self.pointer_lock.take() {
            lock.relative.destroy();
            lock.locked.destroy();
        }
        self.keymap_look(false);
    }

    /// The compositor locked or unlocked the pointer.
    fn keymap_look(&mut self, active: bool) {
        let (Some(object), Some(mapper)) = (self.focused, self.keymap.as_mut()) else {
            return;
        };
        let mut ended = mapper.set_look_active(active);
        if !active {
            // The pointer left the window: nothing the mouse holds should stay down.
            ended.extend(mapper.release_all(Phase::Cancel));
        }
        self.keymap_send(object, &ended);
    }

    /// Relative mouse movement, in the surface's logical pixels.
    fn keymap_look_motion(&mut self, delta: (f64, f64)) {
        let Some(object) = self.focused else {
            return;
        };
        let Some((width, height)) = self.tasks.get(&object).and_then(|task| task.logical_size)
        else {
            return;
        };
        let Some(mapper) = self.keymap.as_mut() else {
            return;
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a motion fraction fits f32"
        )]
        let touches = mapper.look_motion(
            (delta.0 / f64::from(width.max(1))) as f32,
            (delta.1 / f64::from(height.max(1))) as f32,
        );
        self.keymap_send(object, &touches);
    }

    /// Forget mapped contacts after every contact was cancelled elsewhere.
    pub(crate) fn keymap_reset(&mut self) {
        if let Some(mapper) = self.keymap.as_mut() {
            mapper.release_all(Phase::Cancel);
        }
    }

    /// Feed a key or mouse button. Returns true when the mapper took it, so
    /// it must not also go to Android.
    pub(crate) fn keymap_input(&mut self, code: u32, state: InputState) -> bool {
        let (Some(object), Some(mapper)) = (self.focused, self.keymap.as_mut()) else {
            return false;
        };
        let Ok(code) = u16::try_from(code) else {
            return false;
        };
        let outcome = mapper.input(code, state);
        self.keymap_send(object, &outcome.touches);
        self.keymap_sync_lock();
        outcome.consumed
    }

    /// Send mapped touches. A cancel ends every contact on the display, so
    /// one cancel is enough however many contacts ended.
    fn keymap_send(&mut self, object: TaskObjectId, touches: &[TouchOut]) {
        let mut cancelled = false;
        for touch in touches {
            if touch.phase == Phase::Cancel {
                if cancelled {
                    continue;
                }
                cancelled = true;
            }
            let Some(event) = self.keymap_event(object, touch) else {
                continue;
            };
            let result = self
                .synthesized_time()
                .and_then(|time| self.send_input(object, event, time));
            if let Err(error) = result {
                self.fail(&error);
                return;
            }
        }
    }

    fn keymap_event(&self, object: TaskObjectId, touch: &TouchOut) -> Option<InputEvent> {
        let (width, height) = self.tasks.get(&object)?.logical_size?;
        let (x_fixed, y_fixed) = self.fixed_position(
            object,
            (
                f64::from(touch.x) * f64::from(width),
                f64::from(touch.y) * f64::from(height),
            ),
        );
        let (action, pressure) = match touch.phase {
            Phase::Down => (TouchAction::Down, u16::MAX),
            Phase::Move => (TouchAction::Motion, u16::MAX),
            Phase::Up => (TouchAction::Up, 0),
            Phase::Cancel => (TouchAction::Cancel, 0),
        };
        Some(InputEvent::Touch {
            action,
            pointer_id: touch.pointer_id,
            x_fixed,
            y_fixed,
            pressure,
        })
    }
}

impl PointerConstraintsHandler for App {
    fn confined(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _confined_pointer: &ZwpConfinedPointerV1,
        _surface: &wl_surface::WlSurface,
        _pointer: &wl_pointer::WlPointer,
    ) {
    }

    fn unconfined(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _confined_pointer: &ZwpConfinedPointerV1,
        _surface: &wl_surface::WlSurface,
        _pointer: &wl_pointer::WlPointer,
    ) {
    }

    fn locked(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _locked_pointer: &ZwpLockedPointerV1,
        _surface: &wl_surface::WlSurface,
        _pointer: &wl_pointer::WlPointer,
    ) {
        self.keymap_look(true);
    }

    fn unlocked(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _locked_pointer: &ZwpLockedPointerV1,
        _surface: &wl_surface::WlSurface,
        _pointer: &wl_pointer::WlPointer,
    ) {
        self.keymap_look(false);
    }
}

impl RelativePointerHandler for App {
    fn relative_pointer_motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _relative_pointer: &ZwpRelativePointerV1,
        _pointer: &wl_pointer::WlPointer,
        event: RelativeMotionEvent,
    ) {
        self.keymap_look_motion(event.delta_unaccel);
    }
}

#[cfg(test)]
mod tests {
    use super::profile_path;

    #[test]
    fn profile_names_cannot_escape_the_keymap_folder() {
        assert!(profile_path("com.example.game").is_some());
        assert!(profile_path("../evil").is_none());
        assert!(profile_path("a/b").is_none());
        assert!(profile_path("").is_none());
        assert!(profile_path("a..b").is_none());
    }
}
