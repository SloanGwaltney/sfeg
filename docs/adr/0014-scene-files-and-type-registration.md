# 0014. Scene Files and Type Registration

**Date**: 2026-11-03

**Status**: Accepted

## Context

[0011](0011-no-editor-scenes-are-data.md) makes scenes data, referring to object types by stable string names. SFEG needs to decide how names map to types, which file format scenes use, and how references between objects are written in files.

### Naming and registering types

Loading turns a type name in a file, such as `"Enemy"`, into a `Box<dyn GameObject>` (see [0004](0004-use-game-objects-in-scenes.md)). Rust has no runtime reflection, so the game must provide that mapping.

A macro on the type can give it a name, such as `#[sfeg(name = "Enemy")]` generating an implementation of a naming trait. But a type can't add itself to a global list without linker tricks such as `inventory` or `linkme`, which 0004 rejected. So registration is still an explicit call that lists the types, such as `registry.register::<Enemy>()`. The name comes from the type.

Defaulting the name to the Rust type name would mean renaming a struct silently breaks every file that refers to it. That conflicts with the stable names promised in 0011.

### File format

Rust's standard library parses no data formats. All of them come from crates, and all of them plug into `serde`, so types derive `Serialize` and `Deserialize` once and the format is a separate choice.

- **JSON** (`serde_json`): Universal, with strong tooling such as JSON Schema for autocomplete and validation in editors. Verbose, and plain JSON allows no comments, though JSON5 and JSONC add them. JSON has no enums, so serde encodes them by convention, and the same enum takes a different shape per variant: `{"Coin": 5}`, `{"Key": {"door": "red"}}`, `"Nothing"`. It has only `null` for missing values, so `Option<Option<T>>` is ambiguous. Arrays stand in for both tuples and lists, and map keys must be strings.
- **TOML** (`toml`): Clear for flat config. Scenes are trees, and nested children become hard-to-read headers such as `[[objects.children.children]]`.
- **RON** (Rusty Object Notation, `ron`): Built for serde's data model, which is Rust's. Enums read as written (`Coin(5)`, `Key(door: "red")`, `Nothing`), `Option` is `Some` and `None`, tuples and lists are distinct, map keys can be any type, and field names are unquoted. It allows comments and trailing commas. Bevy uses it for scenes. It is little known outside Rust and has weaker tooling.

SFEG's audience is programmers (0011), who are the likely authors of scene files, which suits RON. Human-readable files are also easier to debug.

### References between objects

Runtime handles are a slot index plus a generation (0004), which mean nothing outside the current run. Scene files need their own way to refer to objects.

- **Parent and child** relationships are given by nesting objects in the file, so they need no IDs.
- **Other references**, such as a lever's target door, need a name. Only objects something refers to need one.

Permanent unique IDs on every object, like Unity's GUIDs, were rejected. They are extra state that lives outside the file, and they break when a file is deleted or a change is reverted in version control. They mainly serve editors, which 0011 rules out.

Keeping names at runtime would allow lookups such as `ctx.find("vault_door")`, like Godot's `get_node` or Unity's `GameObject.Find`. Those are string references that break silently on rename, and they fit an editor workflow more than SFEG's. Objects already refer to each other by handle.

### Compatibility and saves

When a game adds or renames a field, existing files still have the old shape. Serde's `#[serde(default)]` and `#[serde(alias = "old_name")]` cover the common cases. Versioned files and migrations are a large scope, and whether the engine should handle them at all is an open question.

Engines like Unity and Godot leave save games to the game. Whether SFEG writes save games is a separate decision. One argument for engine help: the engine owns transforms, the hierarchy, and handles ([0013](0013-object-hierarchy-and-transforms.md)), so a game can't easily snapshot them itself.

## Decision

### Type names and registration

- **Each serializable type gets its name from a macro on the type**, such as `#[sfeg(name = "Enemy")]`, which implements a naming trait.
- **The name is always given explicitly.** It is never taken from the Rust type name.
- **Types are registered with an explicit call per type** that takes the name from the type, such as `registry.register::<Enemy>()`. A macro listing several types may be offered.
- Registering two types with the same name is a typed error at startup.
- Game object types and scene state types (see [0012](0012-game-state-and-scene-state.md)) are named and registered the same way.

### Formats

- **Scene files use RON.**
- **Config files, such as game settings, use TOML.**
- JSON support is not planned. Adding it needs its own decision, weighing the loss of RON's native enums and `Option`.
- A binary format is not planned.

### Scene file structure

- Each object in a file has engine keys at the object level: its type name, an optional ID, its local transform, and its children. The game's own fields are nested under `data`, so game fields never collide with engine keys and new engine keys never break game types.
- **The hierarchy is given by nesting** children inside their parent.
- **Other references use IDs**, plain strings unique within one file. Only objects that are referred to need an ID.
- **IDs exist only at load time.** The loader spawns every object, resolves IDs to handles, then discards the IDs.
- A reference to an undeclared ID, or a typed handle field whose target has the wrong type, is a typed load error.
- A scene file may describe its scene state.
- **There are no permanent unique IDs** and no runtime lookup by name.

### Compatibility and saves

- Keeping old files loading after a type changes relies on serde attributes such as `default` and `alias`. File versioning and migrations are left for later, if they are handled at all.
- Whether the engine writes save games is left to a later ADR. If it does, saves use the same format as scene files.

## Consequences

- Each serializable type needs a name attribute and a registration line. Forgetting either is an error, not a silent fallback.
- Renaming a Rust type never breaks files. Changing a registered name does, which makes renames a visible choice.
- Scene files read like Rust, including enums and `Option`. Non-Rust modders and outside tools must learn RON.
- Moving a scene file, deleting another file, or reverting in version control can't break references, since every reference is local to its file.
- References across scene files aren't possible.
- At runtime, objects find each other only by handle.
- Unsaved fields, marked with `#[serde(skip)]`, take their default value on load.
- Larger changes to a type can break old files until versioning is decided.
- How scene files refer to assets, and how the game's own scene files are checked at compile time (see 0011), are left to the resource loading ADR.

## Notes
