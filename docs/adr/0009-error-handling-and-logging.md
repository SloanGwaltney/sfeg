# 0009. Error Handling and Logging

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG needs a policy for when the engine panics and when it returns errors, how errors are typed, what happens when game code panics, and how logging works. This shapes every engine API and ties directly to the safety pillar.

### When the engine panics

The guiding principle is that the engine should only panic when an outcome is truly unrecoverable by the game. Most failures that look fatal aren't:

- **Missing assets**: A game can handle a missing texture by showing a placeholder or not drawing it. The engine shouldn't assume what the game wants.
- **Lost or outdated surfaces**: These happen routinely in wgpu, such as when a window is resized or minimized. The renderer recreates them.
- **GPU device loss**: Often recoverable by recreating the device and re-uploading assets. The core keeps decoded asset data (see [0006](0006-split-engine-and-game.md)), so the renderer can re-upload without the game knowing.
- **No usable GPU at startup**: The game can't run, but it can still show the player a message. This can be an error returned from `sfeg::run` instead of a panic.

That leaves panics for broken invariants. If the engine reaches a state where it has to panic because of something the game did, that is a broken state, and the behavior must be fixed so the engine returns an error or prevents the state. So panics come from two sources: engine bugs, which are fixed when found, and the game's own panics, such as its own `unwrap()` calls.

Expected runtime failures are already values elsewhere: dead handles return `Option` (see [0004](0004-use-game-objects-in-scenes.md)). Where possible, failures are moved to compile time instead, such as checking at build time that assets exist (see [0001](0001-use-rust.md)).

### Typed errors

`anyhow` is a crate that provides one catch-all error type that can wrap any error and add context. It suits applications that just want `?` to work everywhere, but callers can't `match` on its error kinds, so libraries usually avoid it. The engine returns typed errors instead, such as `AssetError::NotFound { path }`, so games can match on them and decide what to do. Games that want `anyhow` can still use it, since typed errors convert into it with `?`.

### Panics in game code

Unity and Godot log script errors and keep running, and the engine could do the same with `catch_unwind`, disabling or removing the object that panicked. This has real appeal: one buggy enemy doesn't end a player's session. But it is a poor fit for Rust:

- In C#, exceptions carry recoverable errors too, so catching them makes sense. Rust puts recoverable errors in `Result`, so a panic means a bug has happened.
- Continuing would run on state that may be corrupt. An object that panicked mid-tick may have half-updated itself or others, and with take-out during tick (see 0004) it isn't even in its slot.
- Silently removing an object changes game state, which breaks the goal in [0008](0008-fixed-tick-and-per-frame-callbacks.md) that headless and windowed runs behave the same, and it hides the bug.
- `panic = "abort"` is a common release setting, under which `catch_unwind` does nothing, so the engine couldn't rely on it anyway.
- `catch_unwind` requires `UnwindSafe`, which would mean wrapping game objects in `AssertUnwindSafe` and promising something the engine can't check.

A panic should crash, but crash well. The panic hook never touches game state or the engine, which may be corrupt. It works only from a crash report of plain data: the panic message, location, backtrace, and engine context such as the tick number, the type and handle of the object being run, and the current scene. The engine keeps that context up to date in a small thread-local as it runs, so the hook reads a copy instead of the scene. A crash dialog doesn't need the renderer, since a native message box doesn't touch wgpu.

A panic hook can't stop a panic: when it returns, the panic continues. The engine never calls `catch_unwind`, so nothing in the engine catches the panic and returns to the loop. The one gap is background threads: a panic on a thread other than the main one only ends that thread, and the main loop keeps running. Ending the process from the hook after reporting closes that gap regardless of which thread panicked.

### Logging

A logging facade is an API that libraries call to emit log messages without deciding where they go. The application installs one backend at startup, which decides whether messages go to the console, a file, or elsewhere, and filters them by level. `log` is the older, simpler facade, which wgpu and winit use. `tracing` does the same and adds spans (context that wraps messages, such as the current tick and object), structured fields, and collection of `log` messages so wgpu and winit logs appear in the same output.

The engine should own logging, since it needs its own internal logging anyway, and expose it to games. But these facades allow only one global backend per process, installed once. If the engine always installed its own, a game that wanted a different one couldn't install it. So the engine's default must be optional.

## Decision

- **The engine never panics because of anything the game or the environment does through the engine's API.** If the engine reaches a state where it would have to panic because of something the game did, that is a broken state and a bug, fixed by returning an error or preventing the state.
- **Engine panics are reserved for broken internal invariants**, which are engine bugs.
- **Failures the game could handle are returned as errors.** This includes missing assets, which are errors and never panics. The engine doesn't decide how the game responds.
- **Startup failures, such as no usable GPU, are returned as an error from `sfeg::run`.**
- **Recoverable platform failures**, such as lost surfaces and, where possible, lost GPU devices, are handled inside the renderer without involving the game.
- **The engine returns typed errors**, not `anyhow`. Games may use whatever error handling they like on top.
- **The engine may provide placeholder assets**, such as a placeholder texture, that games can choose to use when an asset is missing.
- **Missing assets are guarded against at compile time where possible**, with the details left to the resource loading ADR.
- **Panics in game code crash the game.** The engine never uses `catch_unwind` to keep running.
- **The engine installs a panic hook** that:
  - Builds a crash report from plain data: panic message, location, backtrace, and engine context kept in a thread-local.
  - Writes the report to the log and stderr, and flushes logs.
  - Calls an optional game crash callback with the read-only report, such as to show a native dialog or save the report.
  - Ends the process when done, so no code path resumes, whichever thread panicked.
- **The engine logs through `tracing`.** `sfeg::run` installs a default subscriber unless the game opts out or provides its own. `sfeg` re-exports the logging macros, so games can log with no extra setup.

## Consequences

- Games decide how to handle every recoverable failure, including missing assets.
- A panic caused by game input through the engine's API is always treated as an engine bug.
- Engine APIs that can fail return `Result` or `Option`, which makes some game code more verbose.
- Typed errors let games match on specific failures. Maintaining them is more work than a catch-all type.
- Every panic, from the engine or the game, ends the process. A single buggy object takes down the whole game. This is stricter than Unity and Godot.
- Crashes are reported with engine context, which makes bugs easier to find.
- The crash callback receives only plain data, so it can't touch possibly corrupt state, and anything it does must work without the engine or renderer.
- Ending the process from the hook skips destructors. The hook flushes logs first to make up for it.
- An emergency save from the crash callback isn't supported, since game state may be corrupt.
- wgpu and winit logs appear alongside engine and game logs by default.
- Games that want their own logging backend must opt out of the default subscriber.

## Notes
