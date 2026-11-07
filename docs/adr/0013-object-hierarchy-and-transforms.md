# 0013. Object Hierarchy and Transforms

**Date**: 2026-11-03

**Status**: Accepted

## Context

Every game object has a transform ([0012](0012-game-state-and-scene-state.md)), made of a 3D position, a rotation, and a 3D scale ([0005](0005-normalize-around-3d.md)). SFEG needs to decide where transforms live, how objects nest, and how motion is interpolated, which [0008](0008-fixed-tick-and-per-frame-callbacks.md) left to this ADR.

### Who owns the transform

A transform could be stored by the engine next to each object, or held as a field in the game's struct and exposed through `GameObject` methods.

The engine passes transforms to other systems: the renderer, interpolation, world-transform calculation, serialization, and later physics. If the transform lived in the game's struct, the engine would have to go through the game object to hand its transform to another system. That is awkward, and it misrepresents ownership: the engine truly owns the transform, not the object. With engine storage, every write also goes through the engine, so it knows exactly when a transform changed. Holding the transform in the game's struct would also need two methods per type, or a derive macro.

The cost is ergonomics. Because each object is taken out of its slot during its own tick ([0004](0004-use-game-objects-in-scenes.md)) while its transform stays behind, an object reaches its own transform through the context, such as `ctx.local_mut()`, instead of `self.transform`.

### When the engine changes a transform

[0010](0010-wrap-glam-for-math.md) rejects silent changes to data. A physics engine, though, exists to move things. The line is ownership: the engine never silently changes a transform. A system changes one only when the object has opted into it, such as by having a dynamic physics body, and which system owns the transform is explicit. Engines that handle this well let the body type decide: physics owns dynamic bodies, the game owns kinematic bodies, and nothing moves static bodies. Unity lets game writes to a dynamic body's transform silently fight the simulation.

### Local and world transforms

A child's transform can be local (relative to its parent) or world (its actual place in the scene). Storing local means moving a parent moves its children without touching them, which is the point of a hierarchy. Unity and Godot both store local. Unity's bare `transform.position` silently means world, while `localPosition` means local.

World transforms can be computed after each tick, which leaves other objects' world transforms stale during a tick, or on request by walking up the parents, which is always current. Computing on request costs work proportional to nesting depth, which is negligible at SFEG's scale. A cache that is recomputed only after writes would avoid repeated work for objects that don't move, and can be added without changing the API.

If a parent has non-uniform scale, such as `(2, 1, 1)`, and a child is rotated, the child's world shape is skewed. A position, rotation, and scale can't represent a skew. Forbidding non-uniform scale on parents would hurt 2D games, which stretch sprites often.

### Changing the hierarchy

Spawning and despawning are deferred, because adding or removing slots while the engine loops over them is unsafe (see 0004). Reparenting only changes links inside existing slots, so it can apply immediately. That matters for cycles: making an object a child of its own descendant must be a typed error ([0009](0009-error-handling-and-logging.md)), and a deferred reparent would find the cycle after the tick, with no caller left to return the error to.

### Interpolation

Since the engine owns every transform, it can keep each object's previous transform and interpolate between ticks automatically, so motion is smooth at any refresh rate. This is transform interpolation of position, rotation, and scale, not alpha blending.

- **Interpolating local transforms, then combining them with parents** is correct for hierarchies. A sword in a swinging hand follows the arc of the hand. Interpolating world transforms moves the sword in a straight line, and interpolating world matrices distorts rotations.
- **Some changes must not be interpolated.** A teleport would visibly slide across the map. A reparent that keeps the world transform changes the local transform completely, so interpolating it would swing the object through a wrong position for one frame.
- **Some games don't want interpolation**, such as pixel-perfect 2D games.

## Decision

### Ownership

- **The engine stores each object's transform in the object's slot**, not in the game's struct.
- Objects read and write their own transform through their context, and other objects' transforms by handle, including untyped handles.
- **The engine never silently changes a transform.** A system may change a transform only when the object has opted into that system, and which system owns each transform is explicit.
- A transform can be marked as owned by a system. Game writes that conflict with that ownership are errors or go through an explicit `teleport`, never silent conflicts. Which systems exist and their rules, such as physics, are left to later ADRs.
- The rotation uses the guaranteed-valid rotation type from 0010.

### Local and world

- **Each object stores its local transform**, relative to its parent. For an object without a parent, local and world are the same.
- **Local and world access are both explicitly named**, such as `local()`, `local_mut()`, and `world()`. No accessor has an unqualified name that silently means one or the other.
- Setting a world value, such as `set_world_position`, is an explicitly named operation that sets the local transform needed to reach it.
- **World transforms are computed on request** by walking up the parents, so they are always current. Caching is left to implementation.
- **The world matrix is exact.** World position is always exact. World rotation and scale are approximations when a parent has non-uniform scale, and the docs say so.

### Changing the hierarchy

- **Despawning an object despawns all of its children.**
- **A separately named function detaches the children first**, moving them to the root while keeping their world transforms, then despawns the parent.
- **Reparenting applies immediately and returns `Result`.** A cycle is a typed error.
- **The caller always chooses how to reparent**, through explicitly named functions such as `set_parent_keep_world` and `set_parent_keep_local`. There is no default.
- Spawning and despawning stay deferred (see 0004).
- **Spawning under a parent that is dead when the spawn is applied fails.** The child doesn't spawn, its handle is dead, and the engine logs a warning.
- Objects tick in a fixed order, with no guarantee that parents tick before children.

### Interpolation

- **The engine interpolates transforms automatically.** Before each tick it keeps each object's previous local transform. When drawing, it interpolates between previous and current local transforms by the interpolation factor, then combines them with parents.
- Position and scale are interpolated linearly. Rotation uses spherical interpolation (slerp).
- **Interpolation is skipped for one tick** for an explicit `teleport` or snap, for a newly spawned object, and for a reparent. The docs for the reparenting functions say so.
- **Interpolation is on by default, with a global setting to turn it off.**
- `Draw` still carries the interpolation factor, so games can interpolate their own values.

## Consequences

- The engine can pass transforms to the renderer, serialization, and later physics without going through game objects.
- Objects access their own transform through the context, not as a field on `self`.
- Games don't need to write interpolation code for transforms, and motion is smooth at any refresh rate.
- A teleport must use the explicit call, or the object will slide for one frame.
- Moving a parent moves its children. Game code always states whether it means local or world.
- World transforms are never stale, during a tick or otherwise.
- World rotation and scale can be approximate under non-uniform parent scale.
- Despawning a parent removes its whole subtree. Keeping children alive needs the detaching function.
- Cycle errors are returned to the caller immediately. A reparent is visible to the rest of the tick right away, while spawns and despawns wait until after the tick.
- A spawn under a dead parent fails without an error returned to the caller, since spawns are applied after the tick. It shows up as a dead handle and a logged warning.
- Code that depends on parents ticking before children must not rely on tick order.
- Per-object interpolation opt-out isn't supported. It can be added later as an API extension.
- How transforms and the hierarchy are saved is left to the serialization ADR.

## Notes

- 2026-11-07: The `Transform` type itself is the seam that rejects non-finite positions and scales ([0023](0023-math-value-guarantees.md)): its constructors and setters return `TransformError`, so every stored transform is finite. Combined world matrices are a separate `Matrix4` type, which is always affine and, like vectors, may become non-finite through math.
