// Gemini: never show the on-screen keyboard.
//
// gnome-shell's KeyboardManager decides whether an OSK exists at all with
// (js/ui/keyboard.js, _syncEnabled()):
//
//     enabled = a11ySetting || (seat.get_touch_mode() && lastDeviceIsTouchscreen())
//
// mutter computes touch-mode as `!has_pointer` when the seat has a
// touchscreen and NO tablet-mode switch (src/backends/native/
// meta-seat-impl.c, update_touch_mode()).  The Gemini has a touchscreen,
// a real keyboard, and no pointer, so mutter reports touch_mode=true and
// the shell auto-creates the OSK -- regardless of the accessibility
// toggle.  (A physical keypress does not help: the "last-device-changed"
// handler ignores KEYBOARD devices, so the touchscreen stays the last
// non-keyboard device.)
//
// This extension neutralises exactly that auto path by forcing
// lastDeviceIsTouchscreen() to false; the explicit accessibility OSK
// (screen-keyboard-enabled) still works if it is ever turned on.  We
// additionally lock screen-keyboard-enabled=false in the system dconf
// database, so in practice the OSK never appears.
//
// Uses a private KeyboardManager method, so it may need updating if
// gnome-shell reworks keyboard.js.  Verified against GNOME Shell 50.4.
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

export default class NoOskExtension extends Extension {
    enable() {
        this._origLastDeviceIsTouchscreen =
            Main.keyboard._lastDeviceIsTouchscreen;
        Main.keyboard._lastDeviceIsTouchscreen = () => false;
        // Re-evaluate now; destroys an OSK the auto path may already
        // have created.
        Main.keyboard._syncEnabled();
    }

    disable() {
        Main.keyboard._lastDeviceIsTouchscreen =
            this._origLastDeviceIsTouchscreen;
        this._origLastDeviceIsTouchscreen = null;
        Main.keyboard._syncEnabled();
    }
}
