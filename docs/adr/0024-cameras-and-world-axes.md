# 0024. Cameras and World Axes

**Date**: 2026-11-07

**Status**: Accepted

## Context

The renderer can open a window and clear it, but drawing anything in the world needs a camera. Earlier ADRs constrain it without deciding it:

- [0005](0005-normalize-around-3d.md): cameras carry a projection, orthographic or perspective, and a pixel-perfect orthographic camera should exist for 2D.
- [0004](0004-use-game-objects-in-scenes.md): the engine can ship built-in objects, such as cameras.
- [0017](0017-audio-with-kira.md): the audio listener is set explicitly, such as `ctx.set_listener(camera)`, and follows that object's world transform.
- [0016](0016-input-actions.md): converting the pointer to a world position goes through an explicit camera function.

No ADR fixes the world's axes either: which way is up, which way a camera looks, and the handedness. A camera can't be built without them.

### What a camera is

A camera is defined by a transform (where it is and which way it faces) plus a projection (how the view maps onto the screen). Every game object already has an engine-owned, interpolated transform ([0013](0013-object-hierarchy-and-transforms.md)).

A built-in `Camera` object type is one option, but the game can't give an engine type its own behavior. Following a player, smoothing, shaking, or cutscene rails would all need a game object to drive the camera from outside, such as by parenting the camera to it. If any object can be the camera instead, a game writes its own camera types with their own behavior, and the engine only needs to know which object to look from.

### When there is no camera

A scene can't be required to have a camera when it is created, because the scene starts empty and the camera object doesn't exist yet. The camera object can also be despawned later. Drawing nothing is a plausible-looking failure, an empty screen, so it has to be loud.

### Projections

Perspective and orthographic projections are configured in different units. Perspective uses an angle (field of view), and orthographic uses a size in world units. Some values break the projection math: a zero or negative near plane, a far plane in front of the near plane, a field of view of zero or a half-turn, or a non-finite value. Those are invalid states the engine relies on, so the types guard them (see AGENTS.md, "Same Data, Different Meaning").

### Axes

glam and wgpu, which SFEG uses under the hood, are right-handed with +Y up, and their view conventions look down −Z. Godot and Bevy use the same. Unity is left-handed with +Y up, and Unreal is left-handed with +Z up. Choosing anything other than glam and wgpu's convention would mean remapping values every time they cross into those libraries: extra work and a source of bugs, with no gain for games.

## Decision

- **Any game object can be the camera.** The scene holds an explicit active camera: an object handle and a projection, set by the game, such as `scene.set_camera(handle, projection)`. The view follows that object's interpolated world transform.
- **There is one active camera per scene.** More cameras, such as for split screen or rendering to a texture, are a later extension.
- **With no active camera, the scene draws nothing and the engine logs an error.** This covers a scene that never set one and a camera object that was despawned with no new camera set. It is logged once each time the scene loses its camera, not every frame.
- **Each projection is its own validated type, in its own units**: `Perspective` (vertical field of view, near, far) and `Orthographic` (visible height in world units, near, far). A `Projection` enum holds one of them. Constructors return an error for invalid values and never correct them ([0010](0010-wrap-glam-for-math.md)).
- **The aspect ratio comes from the render target each frame**, so resizing the window never distorts the view.
- **The view uses only the camera object's world position and rotation.** Scale is ignored, so a camera parented to a scaled object isn't distorted.
- **World axes are right-handed, with +Y up, and a camera looks down its −Z axis**, matching glam and wgpu.
- **Moving the camera in `frame`, such as for smoothing or mouse look, is deferred.** A camera moved in `tick` is already smooth through interpolation. Mouse-look latency is decided with input.
- **The pixel-perfect orthographic camera from 0005 is deferred** until 2D drawing needs it.

## Consequences

- Games write their own camera types with any behavior, using the same object, transform, and hierarchy APIs as everything else. A follow camera can be a child of the player, or move itself in `tick`.
- Camera motion is interpolated like any other object, with no extra work.
- Choosing the camera is one explicit call, which matches how the audio listener is chosen in 0017.
- A forgotten or despawned camera shows up as an error in the log, not just a silent empty screen.
- Split screen, picture-in-picture, and render-to-texture cameras need a later extension.
- A projection with invalid values can't be built, so the renderer never has to check for it.
- glam and wgpu values cross into SFEG with no axis remapping. Games coming from Unity or Unreal need to flip their habits around Z or up.
- Camera smoothing and mouse look have to run in `tick` until moving the camera in `frame` is decided. Mouse look in `tick` may feel up to one tick late.
- Games can't use non-uniform or animated camera scale for effects, since scale is ignored.

## Notes
