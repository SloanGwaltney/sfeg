# 0026. Loading Scene Files

**Date**: 2026-11-09

**Status**: Accepted

## Context

[0014](0014-scene-files-and-type-registration.md) decided what a scene file is: RON, an explicit name for each type, explicit registration, nesting for the hierarchy, and IDs that exist only while the file loads. [0015](0015-resource-loading.md) made scene files assets, required asset references in them to be written `Asset("name")`, and loads everything a file refers to into the file's scope. [0025](0025-scene-changes-and-releasing-assets.md) gave the core one next scene, built while the current one runs, and said scene files load into it through the same API.

What is left is how a file turns into objects and into a scene.

### The demo shows the cost of building scenes in code

The demo builds two levels by hand. Each is split in two: everything that needs no texture, then the objects whose textures must first be loaded into the right scope. Most of the demo's length is this kind of building. Scene files are meant to replace it.

### Filling handle and asset fields

A field such as a door's `walker: Handle<Walker>` refers to another object in the same file by its ID, and a tree's `texture: TextureHandle` is written as `Asset("tree")`. Neither value can exist until the loader has reserved the objects' handles and started the texture loading.

- **Derive on the object, with a load context.** The object derives `Deserialize` as usual. While a file loads, the loader holds a context that handle and asset fields read from: IDs map to the handles reserved for them, and assets load into the file's scope. Each object type is one struct, however many instances a file has.
- **A separate data struct and a conversion function.** Nothing is implicit, but every object type is written twice, and every change to a field means changing the conversion too.

Scene files will change often. With the derive, adding a reference means adding a handle field and an ID in the file. With a conversion function, it also means editing code that only copies fields. The load context hides no decision: which scope assets load into is already fixed by 0015.

### Writing references

0014 makes IDs plain strings, unique within one file, and only objects that something refers to need one. It doesn't say how a reference to an ID is written. Written as a bare string, such as `walker: "player"`, a reference looks like any other string field, so only loading the file can find a typo or a renamed ID. Marked the same way assets are, such as `walker: Object("player")`, the build script can find every reference without knowing the game's types and check it against the IDs declared in the same file.

### Turning a file into the next scene

A file may declare the scene state, so the `Scene` value can't exist until the file has loaded. Loading must not freeze the game (0015).

- **One step.** The game asks for the next scene from a file. The engine builds it once the file has loaded, and the game switches when it chooses.
- **Two steps.** The game loads the file as an asset, waits for it, then builds the next scene from it. Every game would repeat the same waiting code.

With one step, the common scene change stays the same as one built in code: begin the next scene, wait until its loads have finished, switch. Building the next scene on its own can't surprise the game, because nothing can see or refer to it before it is built. Switching stays the game's call, so handles into the current scene are only invalidated when the game asks.

### The first scene

Most games will start from a scene file. Blocking while that file is read and built happens before the window opens, so it freezes nothing.

## Decision

### Object types

- **Object and scene state types derive `Deserialize`, plus a macro that names them** (0014), such as `#[derive(Deserialize, SceneObject)]` with `#[scene_object(name = "Door")]`.
- **`Handle<T>` fields are written as `Object("id")`**, naming an ID declared in the same file. The loader resolves them while it loads. An ID whose object isn't a `T` is a typed load error.
- Only objects that something refers to need an ID (0014). A file can hold any number of objects of one type without IDs.
- **Asset handle fields are written as `Asset("name")`.** The loader starts loading each into the file's scope and fills in its handle.
- **Handle and asset fields can only be deserialized inside a scene load.** Anywhere else is a typed error.
- Fields that aren't in the file, such as runtime state, use `#[serde(skip)]` or `#[serde(default)]` (0014).

### Registration

- **The registry of scene types is a required `CoreSettings` field**, like the input bindings.
- Registering two types under one name is a typed error, as 0014 says.

### Scene file contents

- A scene file declares its scene state, its objects as nested trees, and its active camera by ID with a projection.
- Whether a scene file can also be spawned as a subtree, such as a prefab, is left for later. Until then, a scene file is always a whole scene.

### Changing to a scene file

- **`begin_next_scene_from_file(asset)`** begins the next scene from a scene file. It is on the game state and scene state tick contexts, like the other next-scene methods (0025).
- The file loads into the next scene's scope. Until it is built, `next_scene()` returns `None`, and the next scene's load counts include the file.
- **The engine builds the next scene when the tick ends** in which its file finished loading. Its assets then start loading into its scope.
- **The engine never switches on its own.** The game switches with `switch_to_next_scene_deferred` (0025), usually once the next scene's loads have finished.
- Switching to a next scene that isn't built yet is a typed error.
- A file that fails to load or build is reported to the game as a failed load of the next scene, and the game discards it. The current scene keeps running.

### The first scene

- **`Core::new_with_first_scene_file`** starts the game from a scene file. It blocks while the file is read and built, but not while the scene's assets load.
- `Core::new` with a scene built in code stays.

### Build checks

- **`sfeg_build` checks every `Asset("name")` in the game's scene files** against the manifest, as 0015 requires.
- **`sfeg_build` checks every `Object("id")` in the game's scene files** against the IDs declared in the same file. A duplicate ID in one file also fails the build.
- The loader checks the same things, for files the build didn't see, such as mods. Those fail as typed load errors.
- Checking type names and asset kinds in scene files, which needs the game's types, is left to the generated test in 0015.

## Consequences

- Each object type is one struct, with no separate data struct or conversion function. A new reference is a new handle field and an `Object("id")` in the file.
- Handle and asset fields depend on a context the loader holds. Deserializing one outside a scene load fails with a typed error, not a wrong value.
- Renaming or mistyping an ID in one of the game's scene files fails the build. A reference to an object of the wrong type is only caught when the file loads, or by the generated test.
- Changing to a scene file reads the same as changing to a scene built in code: begin, wait, switch.
- The engine builds the next scene without being asked, but the build is invisible until the game switches.
- A game can't build on top of a next scene from a file until it is built.
- Most games start with `Core::new_with_first_scene_file`, and startup blocks briefly on reading the file.
- Prefabs and subtrees from files need another decision.

## Notes
