# 0002. Use wgpu as the Graphics Library

**Date**: 2026-11-01

**Status**: Accepted

## Context

SFEG needs a graphics library. It must be cross-platform and target a single graphics API. Managing several graphics APIs would be major scope creep for an engine of this size.

The main target platform is Windows, but the engine could in theory target any native desktop platform. The web is explicitly not a target (see [0003](0003-web-is-not-a-target.md)). On Windows, wgpu would run on Direct3D 12 or Vulkan; on macOS, Metal or Vulkan; on Linux, Vulkan.

The candidates considered were OpenGL, Vulkan, and wgpu.

- **OpenGL**: Its API has been frozen for almost 10 years. Frozen isn't deprecated, but it won't get new features. Its large global state machine makes it feel non-deterministic, and behavior varies a lot depending on which driver implements it. Some features have a long history of quirky, inconsistent behavior. That might be acceptable when building one game, but not for an engine meant to support many games.
- **Vulkan**: Notoriously verbose, with many moving parts. A thousand lines of code for a triangle is barely an exaggeration. It requires manual management of things like synchronization. For an engine of this size, those extras aren't expected to pay off.
- **wgpu**: Cross-platform by acting as a translation layer over native APIs, including Vulkan. It delivers most of the performance of something like Vulkan with far less API surface to deal with and reason about. It is also written in Rust (see [0001](0001-use-rust.md)), so no binding layer is needed.

## Decision

SFEG will use wgpu as its graphics library.

## Consequences

- One graphics API runs on every target platform, so the engine doesn't have to maintain several rendering backends.
- The API is much smaller and simpler than Vulkan's, which makes rendering code easier to write and reason about.
- Performance should be close to Vulkan's for this kind of project, though the translation layer adds some overhead and less low-level control.
- Behavior is more consistent than OpenGL's, since wgpu avoids OpenGL's large global state machine.
- wgpu is used natively from Rust, with no bindings.
- The engine depends on how wgpu supports each native backend, and on its development and release schedule.
- Brand-new native API features will reach wgpu later than the native APIs. That is acceptable for this project.
- wgpu ships breaking API changes often. The engine will pin a specific wgpu version and accept that upgrading may force rewrites of parts of the engine.
- Shaders will be written in WGSL, wgpu's shading language, rather than GLSL or HLSL. There are fewer tutorials and examples for WGSL.

## Notes
