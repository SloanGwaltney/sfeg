# 0016. Input Actions

**Date**: 2026-11-03

**Status**: Accepted

## Context

SFEG needs an input model. [0008](0008-fixed-tick-and-per-frame-callbacks.md) already gives separate tick and frame input views, with "just pressed" firing exactly once in each, and [0006](0006-split-engine-and-game.md) has input enter the core as plain data from the platform layer.

### Raw input vs. actions

Raw input, such as `key_just_pressed(Key::Space)`, bakes bindings into code. Players can't rebind keys, and supporting gamepads means writing every check twice. Rebinding is expected in modern games and matters for accessibility. Actions, such as `just_pressed::<Jump>()`, let game code say what the player wants, with bindings kept in config.

Offering raw input alongside actions makes it easy for a game to skip actions, and any game that relies on raw input locks out rebinding. Making actions the only path gives every game rebinding by default.

Going without raw input leaves a few real needs that must be covered explicitly, so nobody reaches for a workaround:

- **Rebinding menus** must catch whatever the player presses. That is capturing a binding, not gameplay input.
- **Text entry**, such as a player name, needs characters, keyboard layouts, and IME (composing text in languages such as Japanese or Chinese), not key states.
- **Pointer position**, such as aiming with the mouse, is a screen position, not a button.

### Naming actions

[0004](0004-use-game-objects-in-scenes.md) keeps engine types free of a game-specific generic parameter, so the game's actions can't be an engine type parameter. String names in code, such as `just_pressed("jump")`, fail silently on typos. If one enum held every action, using an axis action as a button would compile and fail at runtime. One type per action makes every misuse a compile error.

Binding files refer to actions by name. Taking those names from Rust type names would mean renaming a type silently breaks every player's saved bindings, the same problem [0014](0014-scene-files-and-type-registration.md) solved for scene types.

### Kinds of input

Controls produce different kinds of values: buttons, single axes, 2D axes, pointer positions, and movement deltas. Delta is separate from pointer because mouse look wants raw motion even when the cursor is locked.

Combining keys into axes defines a value, so the rules must be stated. Four keys pressed diagonally give `(1, 1)`, with length 1.41, making diagonal movement 41% faster. Most modern games normalize this. Sticks never rest at exactly zero, so small values are ignored within a deadzone, which players may need to adjust for accessibility.

### Devices and players

winit doesn't handle gamepads, so another crate is needed. In a single-player game, any device should control the player, including switching devices mid-game. Showing the right button icons, such as "Press Space" vs. "Press A", needs the last used device.

Local multiplayer needs per-player action state and explicit device assignment. It suits party games, but it is hard to justify for most modern games. Online multiplayer is a much larger problem. Neither is on the near horizon.

### Keys, mouse, and focus

On a French AZERTY keyboard, the key in the WASD position is labeled Z. Binding physical key positions puts default controls in the same place on every layout. winit reports both the physical key and its label on the player's layout.

When the window loses focus, it stops receiving key-release events, so a key held while switching away would stay pressed forever.

## Decision

### Actions only

- **All game input goes through actions. There is no raw input API.**
- **Each action is its own type**, declared with a derive that gives it an explicit name and kind, such as `#[action(name = "jump", kind = Button)] struct Jump;`. Code refers to actions only by type, such as `just_pressed::<Jump>()`. There are no string names in code.
- **Action names in binding files are always explicit.** They are never taken from the Rust type name.
- Action kinds are **button**, **axis** (`-1.0..=1.0`), **2D axis**, **pointer** (a screen position), and **delta** (motion since the last read). Using an action as the wrong kind is a compile error.
- Actions follow 0008: "just pressed" fires exactly once in each of the tick and frame views, and deltas accumulate separately for each view, so motion is never lost or counted twice.

### Bindings

- **Bindings live in TOML.** The game ships a default bindings file as an asset in the manifest, so its existence is checked at build time (see [0015](0015-resource-loading.md)).
- **Player rebinds are saved separately and layered over the defaults**, so updating the game never overwrites them.
- Bindings hot reload like other assets.
- **Combination rules are stated in the bindings file.** A 2D axis built from four keys is normalized to length 1 by default, with `normalize = false` to opt out. Stick deadzones are set in the bindings file, with a documented default.
- **A binding of the wrong kind**, such as a single key bound to a 2D axis, is a typed error when the bindings load.
- Bindings use physical key positions. For display, the engine provides the key's label on the player's layout.

### Covering what raw input would

- **Binding capture**: a rebinding menu can ask for the next input pressed. It returns a binding value to store, never a key state.
- **Text input** is a separate stream of text, active only while the game requests it, with layout and IME support. It doesn't expose key states.
- The pointer position is in window pixels. Converting it to a world position goes through an explicit camera function.
- Locking and hiding the cursor are platform requests from the core to the runner (see 0006).

### Devices

- **Any device drives the player**, and the engine tracks the last used device for display.
- **Gamepad disconnects and reconnects are notifications.** The game decides what to do, such as pausing.
- **When the window loses focus, every held action is released**, and the game is notified.
- The gamepad crate is chosen later. It sits in the platform layer and feeds the core plain data.
- **SFEG supports a single player.** Multiplayer, local or online, is neither planned nor ruled out. The single-player input API is designed so that a per-player layer could be added later as an extension.

## Consequences

- Every SFEG game supports rebinding by default, which helps accessibility.
- Even prototypes and debug keys need declared actions and bindings.
- Each action is a separate declaration, which is verbose but checked by the compiler at every use.
- Renaming an action's type never breaks saved bindings. Changing its declared name does.
- Players' rebinds survive game updates.
- Diagonal movement is normalized unless a binding opts out.
- Default controls land in the same physical place on every keyboard layout.
- Rebinding menus and text entry use dedicated APIs instead of raw input.
- Alt-tabbing never leaves keys stuck down.
- Local multiplayer games, such as party games, can't be built on SFEG until a per-player layer is added.
- How player rebinds are stored, and which platform requests `FrameContext` may issue (see 0008), are left to implementation.

## Notes

- 2026-11-06: [0021](0021-scripting-with-luau.md) lets mods declare namespaced input actions with default bindings in their manifest. They follow this ADR's rules and appear in the player's rebinding screen.
