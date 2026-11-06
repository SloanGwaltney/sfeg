# 0005. Normalize the Engine Around 3D

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG has to decide whether it is a 2D engine, a 3D engine, or both. The answer shapes transforms, cameras, and especially the renderer, which must serve games the engine doesn't know about (see [0004](0004-use-game-objects-in-scenes.md)).

The core argument is that the math only generalizes one way. 2D is a slice of 3D: an orthographic camera, with z used as a layer. An engine built around 3D can render 2D, but an engine built around 2D can't render 3D without rewriting its transforms, cameras, and renderer. The parts that are hard to change later are the data model (transforms and cameras), not the rendering features built on top of it. Unity is evidence that this works: it started as a 3D engine and has carried a huge number of 2D games.

Treating 2D as a subset of 3D still has real friction, and the engine has to account for it:

- **Sorting**: 2D sprites are mostly alpha-blended, and the depth buffer doesn't help with blending. 2D needs back-to-front ordering by layer, which is a different path from opaque 3D geometry.
- **Units and pixel-perfect rendering**: 2D games think in pixels, and 3D thinks in world units. Unity needed a separate pixel-perfect camera package to fill this gap.
- **Rotation**: 2D rotation is a single angle. 3D needs quaternions, and making 2D users work with quaternions hurts ease of use.
- **2D-specific features**: Tilemaps and 9-slice sprites have no direct 3D equivalent. Batching exists in both: 2D batches sprites, and 3D sorts draws by material or texture and uses instancing.

Godot takes the other approach on purpose, with separate 2D and 3D node types and renderers. That avoids the friction above but means maintaining two parallel systems, which is too much for an engine of this size.

A game can skip features it doesn't need, but an engine that supports 3D eventually has to provide what any 3D game needs: meshes, model loading (such as glTF), lighting, materials, depth, and likely shadows. That is a large renderer scope for a small engine, and it doesn't all need to exist before the first game ships.

## Decision

SFEG will be normalized around 3D:

- `Transform` uses a 3D position, a quaternion rotation, and a 3D scale.
- Cameras carry a projection, either orthographic or perspective.
- Rendering uses depth for opaque geometry and back-to-front ordering for transparent geometry.
- The API provides first-class 2D conveniences on top of the 3D model, such as constructing transforms from x and y, rotating by a single angle, an orthographic pixel-perfect camera, and layer-based sorting. 2D users should not have to deal with z or quaternions unless they choose to.
- The renderer grows step by step. It starts with the features 2D games need, expressed in 3D space: sprites as textured quads in world space, shapes, text, an orthographic camera, and layer sorting. Meshes, lighting, materials, and other 3D features are added later, each as its own step.

## Consequences

- Adding 3D features later means new rendering features, not rewriting transforms, cameras, or the renderer's foundations.
- 2D games run on the same transforms, cameras, and renderer as 3D games. There is one system to maintain, not two.
- 2D games pay a small cost for carrying 3D data, such as a z coordinate and a quaternion per transform. This is acceptable, since peak performance isn't a goal.
- The 2D conveniences are extra API surface the engine must maintain and keep consistent with the 3D model.
- The renderer must handle both depth-tested opaque geometry and sorted transparent geometry from the start.
- Pixel-perfect 2D rendering needs deliberate support, such as a camera that maps world units to pixels.
- Until 3D features are added, SFEG is in practice a 2D engine with a 3D foundation. Games that need 3D rendering must wait for those features.
- The custom shader and material contract becomes more important as 3D features arrive, and is left to a later ADR.

## Notes
