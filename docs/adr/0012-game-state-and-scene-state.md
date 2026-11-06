# 0012. Game State and Scene State

**Date**: 2026-11-03

**Status**: Accepted

## Context

[0004](0004-use-game-objects-in-scenes.md) makes game objects the only thing that receives engine callbacks. That leaves no place for logic that isn't in the scene, such as a save system, settings, a score counter, or a wave spawner.

Unity has the same gap. Its common workarounds are an empty GameObject placed in the scene only so it receives lifecycle callbacks, or a static singleton whose lifetime the engine doesn't manage. Both put non-scene concerns into the scene or outside the engine. Other engines handle it differently:

- **Godot**: Autoloads, which are still nodes in the tree. The same workaround with better tooling.
- **Unreal**: Subsystems, engine-managed objects that aren't in the world, scoped to the whole game or to one world, with their own startup and shutdown.
- **Bevy**: Resources, data stored once per world and not attached to any entity.

If a game object is by definition a thing in the scene, every game object has a position in it. Non-scene logic then needs a home of its own. That logic comes in two lifetimes: things that last for the whole run, such as settings and a player profile, and things that last for one scene, such as a level's score and objectives.

A registry of separate managers isn't needed. As in [0011](0011-no-editor-scenes-are-data.md), composition through struct fields covers it: several managers are fields of one struct, such as `LevelState { score: Score, waves: WaveSpawner }`.

### Ordering

A fixed order for game state, scene state, and objects within each tick is simple to learn. Splitting it into `pre_tick` and `post_tick` hooks around objects adds flexibility, such as tallying results after every object has moved. But nearly anything a post-tick hook would do can be done at the start of the next tick, so the extra hooks aren't worth learning yet.

### Drawing

Screen-space drawing, such as a HUD or menus, has no world transform, and could be given to game state and scene state through a `draw` callback. It is left out for now: adding `draw` later extends the API, while removing it later would be a breaking change.

## Decision

- **Every game object is a scene object and has a transform.** Game objects are only for things that exist in the scene.
- **Game state** is one game-defined struct that lasts for the whole run. It is created at startup and survives scene changes.
- **Scene state** is one game-defined struct per scene. It is created and destroyed with its scene.
- Game state and scene state are both optional.
- **Game state and scene state have `tick` and `frame` callbacks**, with the same purposes as for game objects (see [0008](0008-fixed-tick-and-per-frame-callbacks.md)). They have no transform and no `draw`.
- **Several managers are fields of one state struct**, not separately registered objects.
- **Each tick runs in a fixed order**: game state, then scene state, then objects. There are no `pre_tick` or `post_tick` hooks.
- Game state and scene state are stored outside the object arena, so tick contexts can borrow them as separate fields alongside other objects (see 0004).
- Game objects and scene state look up game state and scene state by type. Lookups return `Option`, like handles.

## Consequences

- Non-scene logic never needs a placeholder object in the scene.
- Every node in the scene is a real scene object, so the object hierarchy only has to handle objects with transforms.
- Every object pays for a transform, including invisible ones such as triggers. This is negligible.
- Managers in the same scope share one struct. Games that want them separate split them into fields.
- Scene state runs before objects in every tick, so its decisions, such as spawning a wave, apply in the same tick. Anything that must run after objects waits until the start of the next tick.
- Looking up state by type is a runtime downcast. It fails only if the game asks for the wrong type.
- Game state and scene state can't draw. Screen-space drawing, such as a HUD, waits for a later UI ADR. This is accepted for now: a way to draw UI will exist before anyone uses the engine to make a game. Adding `draw` to them later is an API extension.
- What exactly each context exposes, such as whether game state can reach the current scene's objects, is left to implementation.
- Serializing game state and scene state is left to the serialization ADR.

## Notes
