# 0006. Split the Engine and Game, With a Window-Free Core

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG needs a clear boundary between what the engine provides and what the game provides. [0004](0004-use-game-objects-in-scenes.md) makes most of this simple: the game defines its own game object types and adds them to scenes the engine provides. A scene is a collection of game objects. Resources will likely follow a similar path, with one side loading them and storing them in an engine-owned asset store.

The real question is who controls the game loop.

- **The engine owns the loop**: The engine owns the window, rendering, and input, and once started it runs the loop. This is the easy path for games.
- **The game owns the loop**: The game drives each frame itself. This gives more control and supports cases such as running headless, rendering to an off-screen texture, playing the game through a CLI, or running automated tests without a display.

Both cases are legitimate, but they don't have to be either/or. If the part of the engine that runs game logic doesn't depend on the window or GPU, the engine can own the loop by default while games that need control drive that part themselves.

The windowing library pushes the windowed case toward the engine. winit is callback-driven: the application hands it an `ApplicationHandler`, and `run_app` takes over the thread. On macOS it must run on the main thread. A design where the game owns a windowed loop would fight this. winit's `pump_app_events` allows driving it from an outside loop on Windows, macOS, and Linux, so it remains a possible escape hatch, but not something to design around. A headless loop doesn't use winit at all, so letting the game own the loop there costs nothing.

Automated testing matters a lot for this project. [0001](0001-use-rust.md) expects AI coding agents to write much of the codebase and to compile and test often. A core that can be stepped without a window lets tests such as "spawn a player, run 60 ticks, check its position" run anywhere.

### Platform concerns

If the core never touches the window or GPU, it still needs a way to load textures, draw, and use shaders. The core works with handles and descriptions, and the platform turns them into real GPU resources:

- **Textures**: Loading splits in two. Decoding a file into pixels in memory (for example with the `image` crate) doesn't depend on any platform, so it lives in the core's asset store, which returns a `TextureHandle`. Uploading those pixels to a `wgpu::Texture` is the platform's job: the renderer keeps its own map from handles to GPU textures. Game code only sees the handle. A headless run decodes but never uploads.
- **Drawing**: `draw` doesn't call wgpu. It records draw commands, such as `gfx.sprite(texture, position)`, into a list. The runner hands that list to the renderer, which batches and submits it. A headless run drops the list, or a test checks it. Off-screen rendering is the same renderer with a texture as its target. A `Renderer` trait with real and no-op implementations was considered, but a command list is simpler: it is plain data that can be inspected, needs no `dyn`, and leaves the core unaware of who consumes it.
- **Shaders**: Built-in shaders (sprites, shapes, text) are renderer internals and live only in the platform layer. For custom shaders, the core can store WGSL source as text behind a `ShaderHandle` without depending on wgpu. naga, wgpu's shader compiler, is a standalone crate that can parse and validate WGSL without a GPU.
- **Other platform interactions**: Input state, frame time, and window size go into the core each frame as plain data. Platform requests such as quitting, setting the window title, going fullscreen, or hiding the cursor come out of the core as requests the runner applies after each frame, just like the deferred commands in 0004.

### The renderer must be generic

Once the engine is split from the game, the renderer has to serve games it doesn't know about, so it can't be purpose-built for one game's pipelines. That is true of the engine/game split itself, not of the core/platform layering, which only adds draw commands as the format the renderer takes in.

Generic doesn't have to mean exposing most of wgpu. The renderer supports a defined set of drawing features, which grows step by step as described in [0005](0005-normalize-around-3d.md).

How custom shaders and materials plug into the renderer's pipelines is still open. WGSL supports introspection: naga exposes a shader's bindings, types, and entry points at runtime, wgpu can build bind group layouts from a shader automatically, and crates such as `wgsl_to_wgpu` generate Rust bindings from WGSL at build time. Another approach is a convention, where the engine owns some bind groups (such as camera and globals), the game declares material parameters as a Rust struct, and naga checks that the shader matches. Bevy's `Material` trait with `AsBindGroup` works this way. The choice is left to a later ADR.

### Costs of the split

- Indirection: handles instead of GPU resources, and a command list instead of direct wgpu calls.
- Games can only draw what the draw commands can express. A game that wants raw wgpu access would need an escape hatch, such as a custom render hook in the platform layer, which can be added later.

In return, headless runs and automated tests become possible, everything crossing the boundary is plain data that is easy to reason about, and wgpu's frequent breaking changes (see [0002](0002-use-wgpu.md)) stay inside one layer.

## Decision

SFEG will split responsibilities between the engine and the game, and split the engine into two layers:

- **Game**: Defines its game object types, implements their behavior, and provides a setup entry point that builds the first scene. It adds objects to scenes through the engine.
- **Engine core**: Owns scenes, game objects, the tick and frame contexts, deferred commands, time, and the asset store. It doesn't depend on winit, wgpu, or any other platform type. It exposes `tick()` to advance exactly one tick, `advance(real_dt)` to run the ticks that fit in real elapsed time plus frame callbacks, and a way to collect a frame's draw commands (see [0008](0008-fixed-tick-and-per-frame-callbacks.md)).
- **Engine platform layer**: The first platform runner uses winit and wgpu. It owns the window, real input, and the renderer, and runs the windowed loop. Each frame it turns window events into input data, calls the core's `advance` and collects its draw commands, renders the draw commands, and applies the platform requests the core produced.
- **The engine owns the windowed loop by default.** A game starts with a single call such as `sfeg::run(game)` and never writes a loop. Games that need control, such as headless runs, CLI play, or tests, skip the runner and call the core's `tick` or `advance` themselves.
- **Data goes in, requests come out.** Input, frame time, and window size enter the core as plain data. Platform requests leave the core and are applied by the runner.
- **Drawing produces draw commands**, a list of plain data that the renderer consumes.
- **Assets are decoded in the core and uploaded by the renderer.** The game and core refer to assets only by handle.

## Consequences

- Game code is object types plus a setup entry point. Most games only ever call `sfeg::run`.
- The core can run without a window or GPU, so headless runs, CLI play, and automated tests are possible.
- Tests can advance a game tick by tick and check both scene state and draw commands.
- Rendering to an off-screen texture is a renderer change, not a loop change.
- The core never names winit or wgpu types, so upgrades to either mostly affect the platform layer.
- The renderer must be generic, supporting a defined set of drawing features rather than pipelines built for one game.
- Games can only draw what draw commands express until an escape hatch for custom rendering is added.
- Handles and command lists add indirection compared to calling wgpu directly.
- Games that want to drive a windowed loop themselves aren't supported at first. winit's `pump_app_events` could be added later as an escape hatch.
- The layers map naturally onto separate crates, but the crate layout is left to implementation.
- The custom shader and material contract, resource loading details, and object hierarchy are left to later ADRs.

## Notes
