# 0004. Use Game Objects in Scenes

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG needs a high-level architecture for how a game's objects are stored, updated, and refer to each other. This decision shapes where the boundary between the engine and the game falls, so it must come before that split is decided.

Peak performance is not a goal. Safety and ease of use come first, and the engine must be written entirely in safe Rust (see [0001](0001-use-rust.md)). `unsafe` is acceptable in dependencies but not in the engine.

The candidates considered were a full ECS (Entity Component System), a stripped-down ECS, game-owned state, and game objects in scenes.

- **Full ECS**: Ergonomic at scale, but the parts that make it pleasant to use are hard to build in safe Rust. Queries such as `Query<(&mut Position, &Velocity)>` borrow several component stores at once, some mutably. Safe Rust can't prove those borrows are disjoint, so each store ends up behind a `RefCell`, and conflicts become runtime panics instead of compile errors. Archetype storage needs type-erased columns, and parallel system scheduling has the same borrowing problem as queries. Doing these well needs `unsafe`, which is what Bevy does. Bevy is also the right choice for anyone who wants a performance-focused ECS in Rust, so SFEG has no reason to compete with it. ECS can also be harder to follow for small games, since one "thing" is spread across several components and systems.
- **Stripped-down ECS** (component stores and systems, with no queries or archetypes): Possible in safe Rust, but it still breaks down:
  - A static `World` struct with one field per component store needs no registration, is checked at compile time, and works with the borrow checker. But the engine can't name the game's component types, so engine systems can never see game components, and the game must wrap or extend the engine's `World`. That conflicts with the engine/game split.
  - A type map (`HashMap<TypeId, Box<dyn Any>>`) solves the split, but borrowing two stores mutably at once brings back `RefCell` or take-out-and-put-back, with the same runtime errors as a full ECS.
  - Registering components at compile time through macros isn't supported by Rust itself. Crates that fake it (`inventory`, `linkme`) rely on linker tricks or code that runs before `main`. A macro that generates the static `World` struct is just the static approach with less typing.
  - Even with registration solved, games would still have to join pools by hand (finding entities that have both A and B), clear every pool on despawn, including game pools the engine can't see, and read logic spread across pools and systems.
- **Game-owned state** (in the style of raylib or macroquad): The engine abstracts windowing, rendering, and input, then calls the game's `update` and `draw` with a struct the game owns. It is flexible and works well with the borrow checker, but the game has to manage nearly everything itself. It is aimed at programmers more than ease of use.
- **Game objects in scenes** (in the style of Godot, Unity, and Unreal): Each object is a type with its own state and behavior, owned by a scene. It is easy to understand: a player is a `Player` struct with its own methods for updating and drawing. Classic object-oriented designs where objects hold references to each other don't work well in Rust. They usually end up as `Rc<RefCell<…>>`, which is safe but turns borrow errors into runtime panics. Handles solve this, as described below.

### Storing objects: generational arenas and handles

Here an arena means a generational arena (also called a slot map), not a memory or bump allocator. A scene owns its objects in a `Vec` of slots. Code refers to an object by a handle: a slot index plus a generation counter. When an object is removed, its slot's generation goes up, so a stale handle returns `None` instead of the wrong object. This is the safe-Rust version of a dangling-pointer check.

A handle isn't a pointer and can't be used by itself. All access goes through the scene, which checks the generation. Handles are `Copy` and carry no lifetimes, so objects can store handles to each other without the borrow checker seeing any reference between them. Copying a handle copies the reference, not the object.

Looking up a handle may find the object dead. That is a runtime check and a possible error case. Unreal, Godot, and Unity all accept the same cost: a reference found through the scene can be dead at runtime. SFEG makes the case explicit by returning `Option` instead of crashing or using freed memory.

### Updating objects without fighting the borrow checker

While the engine loops over a scene's objects, it holds a mutable borrow of the object storage, so an object's `tick` can't also get `&mut Scene`. Three patterns solve this together:

- **Context struct**: The engine keeps its services as separate fields (input, time, assets, audio, scene). Rust allows borrowing separate fields independently, so the engine builds a context borrowing each one and passes it to `tick`.
- **Deferred commands**: Spawning, despawning, and scene changes go into a command queue applied after each tick. The arena never changes while it's being looped over, and spawn order is predictable.
- **Take-out during tick**: The engine removes each object from its slot, calls its `tick` with access to the rest of the scene, then puts it back. Objects can reach each other by handle in safe Rust. An object can't look itself up through the scene during its own tick, but it already has `&mut self`.

Resources that live outside scenes, such as textures, sounds, and fonts, use the same handle approach. An engine-owned asset store holds the data. Objects keep handles, not references, and read assets through the context.

### Dynamic dispatch vs. a game-defined enum

Nearly all game object types will be written by the game, not the engine. Two ways to store them were considered:

- **Game-defined enum**: The engine is generic over an object type, such as `Scene<O: GameObject>`, and the game provides an `enum` of its object types. It works, but the generic parameter spreads through nearly every engine type (`Scene<O>`, `Context<'a, O>`, `Engine<O>`). Engine-provided objects need a variant in every game's enum, and each new type adds a variant plus a `match` arm.
- **`Box<dyn GameObject>`**: The engine has no generic parameter. The game adds types by implementing the trait, and engine-provided objects sit next to game objects in the same scene. The cost is a vtable call per callback (`tick`, `frame`, and `draw`), which is negligible at this project's scale. Getting a concrete type back means downcasting through `Any`, which is safe but checked at runtime.

Typed handles keep the dynamic setup and make downcasting painless. A `Handle<T>` wraps an untyped handle with a `PhantomData<T>`. Spawning a `T` returns a `Handle<T>`, and looking one up downcasts to `T` automatically. Since Rust 1.86, `&mut dyn GameObject` can be upcast to `&mut dyn Any` directly if `GameObject: Any`, so no `as_any` boilerplate is needed. The generation check means a typed lookup can only fail because the object is dead, not because the type is wrong. Untyped handles stay available for mixed collections.

## Decision

SFEG will use game objects owned by scenes:

- Game objects implement a `GameObject` trait with the callbacks `tick`, `frame`, and `draw` (see [0008](0008-fixed-tick-and-per-frame-callbacks.md)).
- A scene owns its objects as `Box<dyn GameObject>` in a generational arena. Objects live as long as their scene.
- Objects are referred to by handles (index plus generation). Lookups return `Option`.
- Handles are typed (`Handle<T>`) where the type is known, with untyped handles for mixed collections.
- `tick` receives a context struct that borrows engine services as separate fields.
- Spawning, despawning, and scene changes are deferred through a command queue applied after each tick.
- Each object is taken out of its slot during its own tick, so it can reach other objects in the scene by handle.
- Assets live in an engine-owned store outside scenes and are referred to by handles.

## Consequences

- Everything can be built in safe Rust with no `RefCell`-based runtime borrow panics.
- Game code is easy to read: each object type holds its own state and behavior.
- Game objects carry no lifetimes, since they refer to other objects and assets by handle.
- The engine can ship built-in objects (such as cameras or sprites) that sit next to game objects in the same scene.
- Engine types stay free of a game-specific generic parameter.
- A handle lookup can find the object dead. Games must handle the `None` case, as they must in Unreal, Godot, and Unity.
- Downcasting to a concrete type is a runtime check. Typed handles hide it in most cases.
- Each callback is a dynamic dispatch call, and objects are heap-allocated individually. This costs performance compared to an ECS, which is acceptable.
- Spawns and despawns take effect after the current tick, not immediately. Code that expects a spawned object to exist right away must wait until the next tick.
- An object can't find itself through the scene during its own tick.
- Games that need a performance-focused ECS should use Bevy instead.
- How objects nest (parent/child hierarchy), and the exact split between engine and game, are left to later ADRs.

## Notes
