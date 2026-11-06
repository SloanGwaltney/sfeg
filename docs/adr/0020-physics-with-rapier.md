# 0020. Physics with Rapier

**Date**: 2026-11-05

**Status**: Accepted

## Context

[0013](0013-object-hierarchy-and-transforms.md) left room for systems that own transforms and named physics as the first, and [0019](0019-ui-and-screen-space-drawing.md) left occlusion for projected UI to physics raycasts. SFEG has to decide whether it provides physics at all before choosing a library.

### Ship, refuse, or punt

- **Refusing** leaves physics to the game. A game can already read and write transforms in the slot, so it would not need to reimplement them. But a game-owned physics world means going from the engine through a game object out to the physics world and back every tick, which fights the borrow checker. Only the engine can reject a game write to a transform that physics owns, so a game-side integration can follow 0013's ownership rule but never enforce it. Despawn cleanup and colliders in scene files would also have to be kept in sync by hand.
- **Punting** means writing the transform and hierarchy code with no real system owner to test it against, and finding out later that the seam is wrong. Designing the seam properly is most of the hard thinking anyway.
- **Shipping** is a large scope, but it can be staged.

### 2D and 3D

[0005](0005-normalize-around-3d.md) normalizes everything around 3D. Physics is where that stops working. 2D physics is not 3D physics with an axis locked: rotation is a single angle, bodies have 3 degrees of freedom instead of 6, and the solvers and shape sets differ. Locking axes on a 3D engine works but is overkill and produces edge cases such as jitter on the locked axis. Unity and Godot both ship two separate physics systems.

### Libraries

- **Avian** is an ECS-driven physics engine for Bevy, built on Bevy's ECS internally and in its public API. It has no standalone mode, so it is ruled out.
- **Rapier** (Apache-2.0) builds `rapier2d` and `rapier3d` from one shared source, choosing the dimension with a `dim2` or `dim3` feature, so the two share an API by construction. Since v0.32.0 its public math types are glam, which fits [0010](0010-wrap-glam-for-math.md)'s glam newtypes. It also ships f64 builds (`rapier2d-f64`, `rapier3d-f64`), whichever way 0010's open f32/f64 question goes, and an optional `enhanced-determinism` feature. It uses `unsafe` internally, which is acceptable in a dependency.

### Who owns a transform

Rapier has four body kinds: dynamic, fixed, kinematic position-based, and kinematic velocity-based. Each needs a clear owner for its transform (0013). Rapier bodies live in world space while SFEG transforms are local, so a body under a moving parent can have two owners: the parent's motion and the solver.

Physics shapes don't follow transform scale. Rebuilding a collider handles uniform scale, but non-uniform scale turns circles into ellipses that Rapier can't represent, and non-uniform scale under a rotated parent produces shear that no shape can represent. Non-uniform scale is still common in animation, such as squash and stretch on bones.

### Rate

[0008](0008-fixed-tick-and-per-frame-callbacks.md) already gives SFEG a fixed tick. Unity has a separate physics rate because its main update is variable. A second fixed rate on top of a fixed tick either lands physics results between ticks (non-integer ratios) or gains nothing over Rapier's own substeps (integer ratios).

### Collision filtering

Rapier's raw collision groups and Godot's layers and masks both make two colliders interact only if each one's mask includes the other. Getting one side wrong means they silently don't collide. Unity uses one layer per object and one global, symmetric matrix.

Games also need physics rules that change with game state: a dashing player passes through enemies, a stunned enemy becomes pushable, a player drops through a one-way platform. Because physics steps after every object's tick, a setting changed in a tick applies in that same tick's step, so most of these need no callback. Only rules decided per contact, mid-step, need a hook.

### Events

A 2D platformer character usually has two colliders: a solid body and a small sensor at its feet for ground detection. If an object could only receive events for a single collider of its own, the foot sensor would need a child object that only forwards events, and whether the parent saw them the same tick would depend on object tick order, which [0012](0012-game-state-and-scene-state.md) does not fix.

### Units

Rapier is tuned for meters: its tolerances and sleep thresholds assume objects about one unit across. A 2D game working in pixels makes everything feel floaty and sluggish, one of the most common Rapier complaints. Rapier has a length unit setting for this.

## Decision

### Scope and library

- **SFEG ships physics**, using **Rapier**, wrapped per 0010, in its own crate (`sfeg_rapier`) along vendor lines per [0007](0007-crate-layout.md).
- **Each scene declares its physics dimension, 2D or 3D, explicitly.** There is no default and no mixing within a scene.
- **2D ships first**, since SFEG's first game is 2D. 3D follows behind the same API shape.
- **In 2D, physics owns x, y, and rotation about z.** The game keeps z, such as for draw order. A rotation about any other axis on a 2D body is an error.

### Body kinds and ownership

| Kind | Who moves it | A game write to its transform |
|---|---|---|
| `Dynamic` | Physics | Error. Use `teleport`, forces, or impulses. |
| `KinematicPosition` | The game, by setting the transform | Allowed. Physics derives velocity from it. |
| `KinematicVelocity` | The game, by setting velocity | Error. Physics integrates the velocity. |
| `Fixed` | Nobody | Error. Use `teleport`. |

- **Body handles are typed by kind.** Calls that don't apply to a kind, such as `set_velocity` on a `Dynamic` handle, don't compile. Generic transform writes through the context are checked at runtime.
- **Each game object type declares its body**: `type Body = NoBody`, one specific kind, or `AnyBody`.
  - `ctx.body()` returns a `BodyHandle<Self::Body>` without a `Result`.
  - `AnyBody` is opt-in. Its handle checks the kind at runtime, and only `AnyBody` objects can call `set_body_kind`. The switch happens at the next tick boundary, and the call returns a new typed handle, so the old one is stale (a typed error per [0009](0009-error-handling-and-logging.md)).
- **`Dynamic`, `KinematicVelocity`, and `Fixed` bodies must be roots.** Spawning or reparenting one under a parent is an error. Their children follow them normally.
- **`KinematicPosition` bodies may have a parent.** Their world transform each tick is the kinematic target.
- **A parented `KinematicPosition` body must state how it treats ancestor scale**, with no default:
  - `FollowParentScale`: the collider is rebuilt from uniform world scale. Non-uniform ancestor scale is an error.
  - `IgnoreParentScale`: the body takes only position and rotation from its parent, and its collider keeps its declared size.
- **Scale between a collider and its body must be uniform.** Uniform scale is followed: the collider is rebuilt, and a dynamic body's mass is recomputed. Non-uniform scale there is an error.
- **Each body has an explicit dominance setting**, a bounded type over Rapier's −127 to 127 dominance groups, defaulting to 0 (ordinary physics). A body with higher dominance pushes lower ones and is never pushed back by them.

### Colliders

- **An object has a list of colliders**, each with its own offset, for fixed composite shapes.
- **A collider on a child object joins the compound shape of the nearest body above it.** A collider with no body above it is static.
- **Shapes are separate 2D and 3D enums.** 2D starts with `Circle`, `Rect`, `Capsule`, `ConvexPolygon`, `Segment`, and `Polyline`. A shape of the wrong dimension is a load error. Tilemap colliders wait for LDtk and Tiled support.
- **Each game object type declares its collider parts**: `type ColliderPart = Single`, or an enum such as `PlayerPart { Body, Head, Feet }`. In a scene file, each collider gives its part, checked against the enum at load. Events and query results report the part, so code `match`es on it.
- **A collider can be marked `one_way` with a direction.** The engine keeps a contact only when the other body approaches from the allowed side, using Rapier's contact hooks internally.

### Scene files

- **`body` and `colliders` are engine keys** at the object level, next to type, ID, transform, and children ([0014](0014-scene-files-and-type-registration.md)), not under `data`.
- **The loader checks the file against the type.** A missing body, a body on a `NoBody` type, the wrong kind, or an unknown part is a typed load error. An `AnyBody` object's file must state its starting kind.
- **Code spawns bodies and colliders with builders** taking the same parameters, such as `Body::dynamic().linear_damping(0.1)` and `Collider::rect(..)`.

### Layers

- **Collision layers are named in the physics TOML**, up to 32. A build-time generator produces typed constants, such as `Layer::ENEMY`, as in [0018](0018-surface-shaders-and-materials.md) and 0019.
- **One symmetric collision matrix in the same TOML** decides which layers interact. Listing a pair once is enough. Naming an undeclared layer is a build error.
- **Each collider has exactly one layer**, required with no default. A scene file gives it as a name checked at load.
- **Sensors use the same matrix.** The matrix decides whether two colliders interact. The collider's `sensor` flag decides whether the result is a physical response or only events.
- **A collider's layer can change at runtime.** The matrix is fixed at startup.
- **Mods cannot add layers yet.**

### Changing physics at runtime

- **Game code changes physics through handles from `tick`**, as it changes anything else: layer, dominance, sensor, enabled, body kind for `AnyBody`, forces, velocities, and teleports. Changes apply in that tick's step.
- **Explicit pair exclusion**: `ignore_pair(a, b)` and `restore_pair(a, b)`, plus `ignore_pair_until_separated(a, b)`, which ends on its own once the two stop overlapping. These cover rules about two specific objects, such as dropping through one platform or a spear ignoring its thrower.
- **Game objects get no physics callbacks.**
- **Custom contact hooks are deferred.** If added, they will be a pure function over read-only contact data, registered explicitly, with no access to game objects, since they run in the middle of the step.

### Step and timing

- **Physics steps once per tick, after deferred commands are applied.** The order is game state, scene state, objects, deferred commands, the physics step, then write-back of physics-owned transforms. The step is engine work, not a game hook.
- **The physics `dt` is the tick `dt`.** There is no separate physics rate. Rapier substeps are an explicit setting in the physics TOML, and games that need finer physics raise the tick rate.
- **Physics never runs in `frame`.** `frame` cannot apply forces or move physics-owned bodies.
- Physics-owned transforms are interpolated by 0013's interpolation like any other transform.
- **`units_per_meter` is required in the physics TOML.** It's mapped to Rapier's length unit. A missing value is a typed error at startup.

### Events

- **Events from step N are buffered and polled during tick N+1**, like UI events in 0019. They are not visible in `frame`.
- **`Started` and `Ended`.** Each carries the receiver's collider and part, the other collider, the other object's handle, and the other collider's layer. `Started` also carries the contact point and normal.
- **No "still touching" events and no current-contacts query.** Started and ended are enough. Rapier also reports `Started` for objects spawned already overlapping.
- **Both sides of a pair get the event**, each from its own point of view.
- **A pair that starts and ends in the same step produces both events, in order.**
- **Despawning an object produces `Ended` with reason `Removed`.**
- **Objects receive events for their own colliders**, including colliders on child objects that belong to their body. The child object that owns a collider also receives its events.
- **Scene state and game state can read every event of the step.**
- **The other object's handle is untyped.** Getting the type is an explicit check that returns a `Result`.
- **Event reporting is opt-in per collider.** A pair reports if either collider opts in. Sensors always report.
- **Contact force events** (impact strength) are a separate opt-in per collider with a force threshold.

### Queries

- **Raycasts** (first hit or all hits), **shape casts**, and **point and shape overlap tests**, with separate 2D and 3D APIs.
- **Queries see the last completed step.** A teleport followed by a raycast in the same tick sees the old position. This is documented, not hidden.
- **Queries are available in `tick`, scene state, game state, and read-only in `frame`**, such as for occlusion of projected UI and camera anti-clipping. Results in `frame` lag interpolated visuals by a fraction of a tick.
- **Filters are explicit**: the layer set is required (`Layers::ALL` exists but is never implied), whether sensors count is required, and maximum distance is required (`Distance::UNBOUNDED` exists). Excluding your own body is chosen by name: `ctx.raycast_from_self(..)` excludes it, `physics.raycast(..)` excludes nothing. Directions use 0010's guaranteed unit vector type.
- **Results** carry the collider, the untyped object handle, the `ColliderPart`, the hit point, the normal, and the distance.
- **Rapier's kinematic character controller** (move-and-slide, slopes, steps, ground snapping, riding platforms) is wrapped as an explicit call on a `KinematicPosition` handle, such as `body.move_and_slide(desired, &settings)`, which returns the collisions it resolved.

## Consequences

- 0013's rule that a system owns a transform only when the object opts in is now concrete, and the engine enforces it.
- Typed handles make many physics mistakes compile errors instead of runtime errors or silent fights. This is one of the clearest payoffs of 0010's newtype approach.
- 2D and 3D are separate physics worlds. A scene is one or the other.
- 3D costs mostly wrapper work, since Rapier's API shape carries over, but its implementation is still new work.
- Every game type states its body and collider parts. Types with neither write `NoBody` and `Single`.
- Every game sets `units_per_meter` and names its collision layers before using physics.
- Ragdolls are flat sets of root bodies joined by joints, so despawning a character doesn't cascade to its ragdoll bodies. The game tracks them for now. This is a 3D concern and may be revisited.
- No asymmetric layer rules and no collider on several layers. Dominance, one-way colliders, pair exclusion, and runtime layer changes cover the common cases.
- Per-contact rules beyond one-way platforms are not possible until custom contact hooks are designed.
- Games cannot run game logic and physics at different rates.
- Mods cannot add layers. The scripting ADR should consider this along with mod material types (0018).
- Projected UI occlusion from 0019 can now be built on raycasts.

## Notes

- 2026-11-06: [0021](0021-scripting-with-luau.md) lets mods declare collision layers in the game's free slots, and edit interactions between game layers if the game opts in.
