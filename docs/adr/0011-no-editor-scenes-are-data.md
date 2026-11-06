# 0011. No Editor, Scenes Are Data

**Date**: 2026-11-03

**Status**: Accepted

## Context

Whether SFEG ever has an editor shapes the object model. An editor needs far more than saving and loading scenes, and wanting one later would push toward a different design now.

### Who SFEG is for

SFEG is taking the shape of an engine for solo developers and small technical teams, aimed at programmers more than artists. Most decisions so far point that way: Rust ([0001](0001-use-rust.md)), object types defined in code ([0004](0004-use-game-objects-in-scenes.md)), headless runs and tick-by-tick tests ([0006](0006-split-engine-and-game.md), [0008](0008-fixed-tick-and-per-frame-callbacks.md)), and typed errors and compile-time asset checks ([0009](0009-error-handling-and-logging.md)). Many engines ship without an editor, including Bevy so far.

### What an editor gives programmers

Non-programmers, such as designers and artists, are the main reason an editor has to be complete and polished, since they can't fall back to code. But an editor also helps programmers in three ways, and each has a cheaper answer than building an editor:

- **Spatial work**: Laying out a level is spatial even for a programmer, and typing coordinates and recompiling to see them is slow. External level editors such as LDtk, Tiled, and Blender cover this.
- **Tuning values**: Changing a jump height or enemy speed without recompiling. Rust makes this cost larger than it would be in Go, Odin, C, or even C#, where a project this size can simply be restarted after each change. Hot reloading data files covers this.
- **Inspecting live state**: Looking at an object's fields while the game runs. Hotkey debug menus and logging (see 0009) cover this.

Even an editor built only for programmers would have to solve problems that these cheaper answers already solve.

### What an editor needs

- **Serialization** needs less than an editor. Saving is a `serde` derive per type. Loading needs a way to turn a type name in a file, such as `"Player"`, into a `Box<dyn GameObject>` (see [0004](0004-use-game-objects-in-scenes.md)). That works with the current object model plus a mapping from names to types.
- **An editor** needs reflection: listing an object's fields, their names and types, and editing them at runtime. Rust has no built-in reflection, so it would need a derive macro and a large runtime system, like `bevy_reflect`. The editor itself is larger still.

### Components

A Unity-style component model attaches data and behavior to objects at runtime, so a designer can add a `Health` component in an editor without code. Without an editor, that need mostly goes away. Rust already offers composition at compile time through struct fields, such as `Player { health: Health, sprite: Sprite }`, where `Health` is a reusable type with its own methods. Serializing the whole struct is a `serde` derive. Runtime components would also bring back the borrowing problem from 0004: a component that needs another component on the same object needs take-out or `RefCell`-style workarounds.

### Level tools

External tools such as LDtk, Tiled, and Blender can place instances and attach data to them, such as an `Enemy` at a position with `speed = 3`. They can't define what an object does. Behavior always lives in code.

### Modding

Modding comes in two tiers:

- **Data mods**: New textures, sounds, levels, and tweaked values that use the game's existing object types. Scenes as data make this possible. It needs mod files loaded at runtime and stable names for object types and their fields, since those names become an API for modders.
- **Behavior mods**: New code. The options in Rust:
  - **Native plugins** (loading `.dll` or `.so` files): Rust has no stable ABI, so mods must match the exact compiler version. Loading them needs `unsafe`, and mods get full access to the player's machine. This conflicts with the engine running entirely safe Rust.
  - **Embedded scripting**, such as Lua (`mlua`), Rhai, or WASM (`wasmtime`): Safe within the engine. A script-backed type could implement `GameObject` and forward `tick`, `frame`, and `draw` to a script, so scripted objects sit next to Rust objects in the same scene and register by name the same way.

Mod files can't be checked at compile time, but the game's own data can. The game's own assets and data should still be checked for existence at compile time, as [0001](0001-use-rust.md) and [0009](0009-error-handling-and-logging.md) intend.

## Decision

- **SFEG has no built-in editor.** It is a non-goal, not a feature planned for later.
- **SFEG has no runtime component model.** Objects reuse behavior by holding other structs as fields. [0004](0004-use-game-objects-in-scenes.md) is unchanged.
- **Scenes are data.** Scenes and game data can be described in files, not only in code.
- **The game's own assets and data are checked for existence at compile time** where possible. Loading files that can't be checked at compile time is for mod data.
- **Object types are referred to in data by stable string names.**
- **Data mods are possible by design.**
- **Behavior mods are left to optional embedded scripting**, which may be added later. The scripting language is left to a later ADR.
- **Native plugins are ruled out.**
- **Modding is opt-in per game.** The engine provides the hooks, and each game decides whether to enable them.

## Consequences

- SFEG is aimed at programmers. Non-programmers can't build or change scenes without code or hand-edited files.
- The engine doesn't need a reflection system, which keeps it smaller.
- Programmers lose an editor's fast spatial layout, value tuning, and live inspection. These are covered instead by external level editors, hot reloading, and debug menus and logging.
- Visual level editing, where wanted, comes from external tools whose files the game or engine loads.
- Hot reloading of data files is required to make up for Rust's compile times. The details are left to the resource loading ADR.
- Object composition is checked at compile time, and objects can't gain or lose components at runtime.
- Object type names and their fields become a public API for games that enable modding. Renaming them can break mods.
- Mod data fails at runtime with typed errors, while the game's own data is caught at compile time. The engine must support both paths.
- Adding scripting later means adding a script-backed `GameObject` type, not changing the object model.
- Adding an editor later would require reflection and would likely revisit 0004.
- How names map to types, the scene file format, and how handles and fields are saved are left to the serialization ADR.
- How the game's own files are checked at compile time, how mod files are loaded at runtime, and how mod files layer over base files are left to the resource loading ADR.

## Notes
