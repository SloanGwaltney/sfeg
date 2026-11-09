# 0025. Scene Changes and Releasing Assets

**Date**: 2026-11-09

**Status**: Accepted

**Supersedes**: [0015](0015-resource-loading.md)'s Scopes section, and its rule that during a scene change the new scene's assets load while the old scene runs. The rest of 0015 stands.

## Context

[0015](0015-resource-loading.md) ties every asset to scopes: game, scene, or object. It left two things open:

- **How a scene change works.** 0015 says the new scene's assets load while the old scene runs, and that only assets the new scene doesn't hold are freed when the old one ends. For that, the new scene needs its own scope before it becomes the current one. [0004](0004-use-game-objects-in-scenes.md) says scene changes are deferred until the tick ends. [0012](0012-game-state-and-scene-state.md) says game state survives them and scene state doesn't.
- **Freeing an asset before its scope ends.** 0015 left out manual unloading, with a note that it might be added if a case arose that scopes don't handle.

### The case scopes don't handle

A long-lived object can work through many assets, such as a picture frame cycling through photos, or a material whose texture keeps changing. Each texture stays loaded until the object or the scene ends, so memory grows for as long as it runs. A small, known set, such as three skins, is fine: load them all and swap handles.

0015 rejected a global unload because unloading an asset still in use breaks every other handle to it. Releasing one scope's hold avoids that: the asset stays loaded while any other scope holds it.

Under 0015, a scope only records *whether* it holds an asset. Two goblins in the same scene that both load the goblin sprite make one hold, so if one released it, the other's would go too. Releasing therefore needs each scope to count its loads.

### Where the next scene lives

- **Any `Scene` value carries its own scope**, freed when the value is dropped. This allows building several candidate scenes, but frees assets whenever a scene happens to be dropped, which is an implicit time.
- **The core holds at most one next scene.** Its scope begins and ends at explicit calls, applied when the tick ends.

Predictability matters more than building several scenes at once: the game must always be able to tell when an asset will be freed.

### Who changes scenes

What scene comes next is a game-wide or scene-wide decision. If objects could switch scenes, those decisions would be scattered across objects. An object that ends a level, such as a door, can set a value on the scene state, and the scene state switches.

### Scene files

Most scene changes will come from scene files ([0014](0014-scene-files-and-type-registration.md)). 0015 requires every asset reference in them to be written `Asset("name")`, so the engine can load everything a scene file names into the next scene's scope. Loading single assets into the next scene's scope is the layer under that, and stays public for scenes built in code, procedural levels, and assets chosen at runtime.

## Decision

### Scopes

- **Assets are loaded into one of three scopes: game, scene, or object.** Object scope covers the object's whole subtree, since children despawn with their parent ([0013](0013-object-hierarchy-and-transforms.md)).
- **Every load names its scope.** There is no default.
- **Everything a file refers to is loaded into the same scope as the file.**
- An asset is loaded once. Loading an already-loaded asset returns the same handle.
- **Each scope counts its loads of each asset.** A scope holds an asset while it has loaded it more times than it has released it.
- **An asset is freed when no scope holds it.**
- Handles stay `Copy`. A handle to a freed asset reports it as unloaded.
- The rule for objects is "load what you use": an object using a handle it didn't load loses the asset when the loading scope ends or releases it.

### Releasing

- **A release undoes one load by one scope**, such as `release_texture_from_scene_scope_deferred(texture)`. It never affects another scope's hold.
- **Releases are deferred** and applied when the tick ends.
- Releasing more times than the scope loaded is a typed error, returned when the release is asked for.
- **Global unloading is not provided.**

### Scene changes

- **The core holds at most one next scene** besides the current one. It has its own scene scope.
- **Only the game state and the scene state change scenes.** They can:
  - begin the next scene, which exists right away
  - load assets into the next scene's scope, and spawn objects into it, while the current scene keeps running
  - check how the next scene's loads are going
  - switch to the next scene, or discard it
- **Switching and discarding are deferred** until the tick ends.
- **On a switch**, the current scene, its scene state, and its objects are dropped, and its scope ends. The next scene becomes the current one, and first ticks in the next tick. The game state is kept.
- **On a discard**, the next scene is dropped and its scope ends.
- Beginning a next scene while one exists, or switching or discarding when there is none, is a typed error.
- **Switching doesn't wait for loads.** If the game switches while some are pending, they finish as usual. Whether to wait, such as behind a loading screen, is up to the game.
- Objects can't change scenes, but can set values on the scene state that it acts on.
- Scene files, once they exist, load everything they name into the next scene's scope through the same API.

### When assets are freed

An asset is freed only when the tick ends, and only by an explicit call:

- **A release is applied** and no scope holds the asset any longer.
- **A switch is applied**, which ends the old scene's scope.
- **A discard is applied**, which ends the next scene's scope.
- **An object despawns**, which ends its object scope, once object scope exists.

Assets are never freed in the middle of a tick, or because a value was dropped.

## Consequences

- A game can tell when any asset will be freed by looking at its loads, releases, switches, discards, and despawns.
- Long-lived objects can cycle through assets without memory growing.
- Releasing can't break another scope's handles. Forgetting a release keeps the asset loaded until its scope ends, which is the safe way to fail.
- Two goblins in one scene each loading the goblin sprite count as two loads, so either can release its own.
- Only one next scene can be built at a time.
- A door that ends a level goes through the scene state, which is one more step than switching directly.
- Switching before loads finish shows placeholders for what is still loading. Loading screens are the game's to build.

## Notes
