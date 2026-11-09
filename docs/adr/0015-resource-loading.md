# 0015. Resource Loading

**Date**: 2026-11-03

**Status**: Superseded by 0025

## Context

Several earlier ADRs left parts of resource loading to this one:

- [0001](0001-use-rust.md), [0009](0009-error-handling-and-logging.md), and [0011](0011-no-editor-scenes-are-data.md) want the game's own assets checked for existence at compile time.
- 0011 requires hot reloading data files, and keeps data mods possible.
- [0014](0014-scene-files-and-type-registration.md) leaves how scene files refer to assets.
- [0004](0004-use-game-objects-in-scenes.md) puts assets in an engine-owned store referred to by `Copy` handles, and [0006](0006-split-engine-and-game.md) decodes them in the core and uploads them in the renderer.

### Where assets live

Assets built into the executable with `include_bytes!` ship as one file and can't go missing, but they can't be hot reloaded or overridden by mods. Since hot reload is required and mods should stay possible, the game's assets are files on disk.

### Naming assets

Any reference has to point at something, so renaming a file always has to update something. The options differ in where:

- **Paths in code**, or constants generated from file names: a rename touches every use. This also ties code to file system naming.
- **Permanent IDs**, like Unity's `.meta` files: a rename touches nothing visible, but the IDs are hidden state that breaks when files are deleted or changes are reverted in version control. 0014 already rejected these for objects.
- **Explicit names in a manifest**: a rename touches one line.

0014 refused to take type names from Rust type names. The same reasoning applies to assets: identity shouldn't come from file names. A manifest maps developer-chosen names to files, so moving or renaming a file changes one line and nothing in code or scenes notices.

### Checking references

A build script can check that every path in the manifest exists and generate typed constants for the manifest's names, so a typo in code is a compile error.

Once objects come mostly from scene files acting as prefabs, most asset references live in data rather than code. The build script runs before the game's code is compiled, so it can't know the game's types or tell which strings in a scene file are asset references. Writing every asset reference in RON as `Asset("name")` lets the build script find and check them without knowing types.

Checking whether an asset is what it claims to be, such as a texture field pointing at a sound, needs the game's types. So does checking type names in scene files, which are registered at runtime (0014). That is a testing concern, covered by a generated test that loads every scene file of the game.

### Freeing assets

- **Reference counting**, as in Bevy, frees an asset when its last handle is dropped. Handles could no longer be `Copy`, breaking 0004, and freeing happens at implicit times.
- **Manual unloading** is explicit, but unloading an asset still in use silently kills every other handle to it.
- **Never freeing** is simplest and works for small games, but not for games with many large levels.
- **Scopes** tie assets to lifetimes the engine already has. Game and scene scopes match [0012](0012-game-state-and-scene-state.md). An object scope covers assets unique to one object, such as a boss whose textures, materials, and shaders nothing else uses, freed when the boss dies.

With scopes, one asset may be held by several, such as a boss's roar used by both the boss and his arena. Counting scopes rather than handles keeps handles `Copy` and frees assets only at clear points. A default scope would make object scope easy to use by accident: 50 goblins each holding the goblin sprite in their own scope would free it when the last one dies and reload it for the next wave.

### Sync vs. async

Blocking the main thread while loading freezes the game: no progress bar, no animation, nothing but the last frame shown. Loading must not block the main thread. Determinism mainly matters for tests (see [0008](0008-fixed-tick-and-per-frame-callbacks.md)), and a completion notification covers that: tests wait for the notification instead of assuming a load finished on a given tick.

What to do while loading, and what happens to the previous scene, belongs to the game. Some games stream sections of a level in and out instead of replacing whole scenes. Godot does this by adding and removing nodes in one scene tree, rather than running several scenes at once. That keeps handles, tick order, and contexts tied to a single scene.

### Hot reload

Most assets, such as textures, can be swapped behind their handle with no game involvement. Two cases need care:

- **Config values the game copied**, such as into an object's field at spawn, don't change when the file does.
- **Objects already spawned from a scene or prefab file** have runtime state. Respawning them loses that state. Patching changed fields onto them requires telling apart changes in the file from changes made at runtime, which is complex and easy to get subtly wrong.

Saving a file halfway through an edit is common, so a failed reload must not break a running game. Reloading Rust code needs dynamic libraries, with the same `unsafe` and compiler-version problems as native plugins (0011), and the small speedup isn't worth the work.

### Mods

Since the manifest defines asset identity, mods work in names, not paths. Overriding and adding are different intentions. If both are expressed only by using a name, two mods that each add `"goblin_king"` collide by accident. Namespaces separate added names, and declaring overrides makes them intentional. Declared dependencies between mods can form cycles, which no load order can satisfy. Mutual dependency is never needed: mods meant to work together can be one mod, or share their common parts in a third mod.

### Asset kinds

Each kind of asset needs a way to decode, validate, reload, and possibly upload it to the GPU. Every supported format is a dependency to maintain and a decoder that must handle malformed files safely. Most game-specific data is plain Rust structs that can be read from RON. Some games will need formats the engine doesn't know, such as an unusual texture format.

## Decision

### Naming and checking

- **The game's assets are files on disk.** Engine assets, such as placeholders, may be built into the engine.
- **An asset manifest maps explicit names to an asset kind and a path**, such as `"player_idle": Texture("textures/player-idle (2).png")`. Names never come from file names.
- **Code refers to assets through constants generated from manifest names**, such as `assets::PLAYER_IDLE`. A typo is a compile error.
- **Asset references in RON are always written `Asset("name")`.**
- **The build fails** if a manifest path doesn't exist, or if an `Asset(...)` reference in the game's scene files doesn't match a manifest name.
- **A generated test** checks what the build can't: asset kinds against the fields that use them, and type names in scene files. It can be added later.
- Names that aren't known at build time, such as mod assets, go through an explicit runtime path that returns `Result`.

### Scopes

- **Assets are loaded into one of three scopes: game, scene, or object.** Object scope covers the object's whole subtree, since children despawn with their parent ([0013](0013-object-hierarchy-and-transforms.md)).
- **Every load names its scope.** There is no default.
- **Everything a file refers to is loaded into the same scope as the file.**
- An asset is loaded once. Loading an already-loaded asset returns the same handle and adds a scope holding it. **An asset is freed when every scope holding it has ended.**
- Handles stay `Copy`. A handle to a freed asset reports it as unloaded.
- The rule for objects is "load what you use": an object using a handle it didn't load loses the asset when the loading scope ends.
- Manual unloading is not provided. It may be added as a fallback if a case arises that the scopes don't handle.

### Loading

- **All loading is async. The engine never blocks the main thread on loading.**
- **The game is notified when a load finishes**, at a tick boundary.
- An asset's state is explicit: `Loading`, `Ready`, `Failed` with a typed error, or `Unloaded`.
- **"Ready" means decoded in the core.** Uploading to the GPU is the renderer's concern and invisible to the game.
- What happens while waiting, such as showing a loading screen, is up to the game.
- Headless runs and tests can call an explicit core function to finish pending loads.
- **Scene files are assets.** Once one is ready, the game either spawns it into the current scene as a subtree, or makes it the new scene. One scene is active at a time.
- A file that declares scene state can only be used as a scene. Spawning it as a subtree is a typed error.
- During a scene change, the new scene's assets load while the old scene runs. When the old scene ends, only assets the new scene doesn't hold are freed.

### Hot reload

- **The engine watches asset files and swaps changed assets behind the same handle** at a tick boundary, then notifies the game.
- Assets such as textures update with no game involvement. Config values the game copied don't change, and the game is notified so it can respond.
- **Objects already spawned from a changed scene or prefab file are left alone.** Future spawns use the new version, and the game is notified. A respawn helper may be added later.
- **A failed reload keeps the old version**, logs the error, and notifies the game with it.
- Hot reload is a setting, on by default in debug builds and off by default in release builds.
- **Hot reloading code is ruled out.**

### Mods

- **Mods are applied at startup only.**
- Each mod declares an ID in a `mod.toml`. It is never taken from the folder name. Two mods with the same ID is a typed error.
- A mod's manifest has separate `add` and `override` sections.
- **Names a mod adds live in its namespace**, such as `hd_goblins:goblin_king`, so mods can't collide by adding.
- **Overrides are declared explicitly.** Overriding a name that doesn't exist, or changing its kind, is a typed error.
- Names are always resolved without fallback. Inside a mod, an unqualified name is that mod's own, `game:` names the base game's, and `other_id:` names another mod's.
- **The enabled mods are an explicit ordered list** in a TOML file. When two mods override the same name, the later one wins, and the conflict is logged.
- A mod that refers to or overrides another mod's names declares it as a dependency. The dependency must be enabled and come earlier in the list.
- **A dependency cycle is its own typed error naming every mod in it.** Every mod in the cycle fails to load.
- **Mods load all or nothing.** At load, a mod gets the same checks the build gives the base game. A mod with any error fails as a whole, and so does every mod that depends on it. The game receives the typed errors and decides whether to continue.
- Mod files are untrusted input. Malformed files produce errors, never panics.

### Asset kinds

- **Each built-in kind supports one format to start**, unless there is a clear need for more. Textures use PNG, scenes use RON, and config uses TOML. Formats for sound, fonts, and shaders are left to their ADRs. glTF is supported when 3D support grows.
- **Any registered type that derives `Deserialize` can be loaded from RON as a data asset**, such as `"goblin_stats": Data("EnemyStats", "data/goblin.ron")`. Data types are named and registered as in 0014.
- **Games can add a custom loader** through a loader trait, registered explicitly with a name. A custom loader may produce a game type or a built-in kind, such as a texture from an unusual format.
- Loaders run on background threads, so they must be thread-safe, and they return errors instead of panicking.
- Loaders for external level editor formats, such as LDtk and Tiled, are left for later.

## Consequences

- Renaming or moving an asset file changes one line in the manifest. Code and scene files never notice.
- Each asset needs a manifest entry. A file missing from the manifest can't be used.
- Typos and missing files in code and the game's scene files fail the build. Wrong asset kinds are caught by tests, not the build.
- Scene files must mark asset references with `Asset(...)`.
- Tuning which assets an object uses is a data change, and with hot reload needs no recompile.
- Every load call names a scope, which is more verbose but makes every asset's lifetime visible.
- An object that uses a handle it didn't load can lose the asset when another scope ends.
- Game code must handle `Loading` and `Unloaded` states, not only `Ready` and `Failed`.
- Loading never freezes the game. Loading screens and streaming are up to the game.
- Streaming level sections means spawning and despawning subtrees, not running several scenes.
- Values copied from config and objects spawned from changed scene files don't update on reload. Games that want live tuning read from the asset or respond to the notification.
- A half-saved file never breaks a running game.
- Code changes need a rebuild and restart.
- Mods can't collide by adding names, and every override is intentional. When two mods override the same name, the result depends on load order.
- Changing the enabled mods requires a restart.
- A broken mod disables itself and its dependents, not the game.
- Supporting a format beyond the one per kind means writing a custom loader, until the engine adds it.
- If scripted mods arrive, the type names they register will need the same namespacing. That is left to the scripting ADR.

## Notes

- 2026-11-06: "Hot reloading code is ruled out" refers to Rust code. [0021](0021-scripting-with-luau.md) adds hot reload for mod scripts when the game opts in. Scripts don't register type names, but the behaviors, input actions, and collision layers that mods declare are namespaced as this ADR describes.
- 2026-11-09: [0025](0025-scene-changes-and-releasing-assets.md) supersedes only this ADR's Scopes section and its rule for loading during a scene change. It adds per-scope load counts, releasing one scope's hold, and the next-scene API. Everything else here stands.
