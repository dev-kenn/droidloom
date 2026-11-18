# Keyboard and mouse mapping

Droidloom can turn keyboard and mouse input into touch for games that have no
keyboard support. A profile for a game says which keys touch which points. It
is a plain text file you edit by hand.

## Where profiles live

`$XDG_CONFIG_HOME/droidloom/keymaps/<package>.toml`, or
`~/.config/droidloom/keymaps/<package>.toml` when `XDG_CONFIG_HOME` is unset.
`<package>` is the Android package name, for example `com.example.game`. The
`package` line inside the file must match. A copy to start from is in
`profiles/example.toml`.

The presenter reads the profile each time the game window gains focus. Edit
the file, then click back into the window.

## What a profile can do

- **`[[tap]]`** a key or mouse button touches one point while held, or up to
  four points with `points = [[x, y], ...]`.
- **`[wheel]`** four keys steer one virtual joystick touch. Opposing keys
  cancel, and diagonals use 0.707 of each axis.
- **`[look]`** mouse movement drags one touch around a region. The pointer is
  locked while the game has focus. The touch lifts and restarts at its anchor
  when it leaves the region.
- **`[[aim]]`** hold a key to put a touch down at a point and steer it with
  the mouse, for aimed skills or a view mode. An optional cancel key moves the
  touch to a cancel point and lifts it.
- **`toggle_key`** switches mapping on and off. Turning it off gives the
  mouse back and lifts every mapped touch.

Positions are fractions of the window (0.0 to 1.0). Up to ten mapped touches
can be held at once. A key may be used only once in a profile. Keys that are
not in the profile go to Android as normal.

Key names are case-insensitive: letters, digits, `F1` to `F12`, `Space`,
`Tab`, `Enter`, `Escape`, `LeftShift`, `LeftCtrl`, `LeftAlt`, `CapsLock`, the
mouse buttons `MouseLeft`, `MouseRight`, `MouseMiddle`, `MouseSide`,
`MouseExtra`, or an evdev number as `code:N`.

## How it is built

- `graphics/droidloom-keymap/` holds the profile parser and the mapper. It
  has no Wayland or Android dependency, so it builds and tests anywhere:
  `cargo test -p droidloom-keymap`.
  - `profile.rs` reads and checks profiles.
  - `keys.rs` maps key names to Linux evdev codes.
  - `mapper.rs` turns key and mouse events into touch records (down, move,
    up, cancel) with positions in window fractions.
- `graphics/droidloom-wayland/src/keymap_input.rs` connects the mapper to the
  presenter: it loads the focused game's profile, sends the touch records
  through the same input path as real touches, and holds the pointer lock.
  `main.rs` calls into it from the key and pointer handlers.
- The mapper uses touch identities starting at `0x8000_0000`, so they never
  clash with real touch contacts.

## Limits

- Mouse-look needs a compositor that supports `zwp_pointer_constraints_v1`
  and `zwp_relative_pointer_manager_v1`. Without them the presenter prints a
  message and mapping still works for keys and buttons.
- Mapped keys are not also sent to Android.
- The left mouse button is a normal mouse-as-touch click unless a profile
  with mouse-look is active, in which case it can be mapped as a tap.
