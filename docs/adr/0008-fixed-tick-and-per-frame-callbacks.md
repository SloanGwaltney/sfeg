# 0008. Fixed Tick and Per-Frame Callbacks

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG has to decide how game time advances: whether game objects update once per rendered frame with whatever time has passed, at a fixed rate unrelated to rendering, or both.

- **Variable timestep** (update once per frame with the frame's `dt`): Simple, and motion is smooth at any refresh rate. But the same inputs give different results at different frame rates, so tests and replays aren't reproducible. A hitch or window drag produces a huge `dt` that can make objects tunnel through walls or make physics explode. Every piece of game code has to scale by `dt` correctly, and mistakes become frame-rate-dependent bugs.
- **Fixed timestep** (update at a set rate, such as 60 Hz): The runner keeps an accumulator of real time and runs as many fixed steps as fit each frame. The same inputs give the same results on the same build and machine (cross-platform floating-point determinism is a separate problem). Tests are exact, physics is stable, and game code can treat `dt` as a constant. The costs: when the render rate doesn't match the tick rate, some frames run zero ticks and some run several, so motion judders unless it is interpolated between the previous and current state. If ticks take longer than real time, the accumulator grows without bound (the "spiral of death"), so ticks per frame must be capped. Input "just pressed" events have to fire exactly once even when a frame runs zero ticks or several.
- **Hybrid** (a fixed callback for game logic and a per-frame callback for visuals, as in Unity and Godot): Flexible, and lets visual work such as camera smoothing run at the display's rate. The common complaint is confusion about which callback code belongs in. Part of that confusion comes from naming: Unity's `FixedUpdate` describes how often it runs, not what it is for, and plain `Update` invites putting everything there. Names that describe purpose make the choice much clearer.

The hybrid fits the stepped core from [0006](0006-split-engine-and-game.md): advancing a headless game by one second at 60 Hz means running 60 ticks. But it has a real risk there. Headless runs and tests advance by ticks only. If game logic ends up in the per-frame callback, headless runs silently behave differently from windowed ones. The rule has to be that the tick changes the game and the per-frame callback only presents it. Rust's types can enforce most of that by giving each callback a different context: the per-frame context has no command queue and no access to other objects, so spawning or changing other objects from it is a compile error. An object can still change its own fields per frame, so the rule can't be fully enforced, but the common mistakes become impossible.

### Why per-frame updates and drawing are separate

Per-frame updates and drawing could be one callback, but keeping them separate has three benefits:

- **Ordering**: Every object's per-frame update runs before any object draws, so all presentation state is settled before drawing. If they were merged, drawing would happen in update order, and an object could be drawn from a camera that hasn't been updated yet, or a health bar could draw a value another object hasn't updated.
- **Drawing can't change state**: `draw(&self)` is read-only, enforced by the borrow checker. The engine can safely call it zero times (headless, minimized), more than once per frame (split-screen, minimaps, render-to-texture for screenshot tests), or without advancing anything (redrawing on resize while paused).
- **One job each**: `tick` changes the game, `frame` updates how it looks, and `draw` reports what to render.

The cost is one more trait method. With default empty implementations, objects only implement the callbacks they use.

### Input

The window delivers input per frame, so the engine keeps two views:

- **Tick input**: "just pressed" means since the last tick. A press between ticks shows up on the next tick exactly once, even if the frame it arrived in ran zero ticks. In a frame that runs several ticks, only the first sees it.
- **Frame input**: "just pressed" means since the last frame, for low-latency uses such as mouse look and UI.

### Naming the core's tick

The core's method for advancing one tick is also named `tick`. Matching the callback name could seem confusing at first, but it describes the same thing: one call to the core's `tick` runs one tick of every object.

## Decision

SFEG will use a fixed tick for game logic, plus separate per-frame and drawing callbacks:

- The `GameObject` trait has three callbacks, each with a default empty implementation:
  - `tick(&mut self, ctx: &mut TickContext)`: Runs at a fixed rate and changes the game. `TickContext` provides the command queue, access to other objects by handle, tick input, and the fixed `dt`.
  - `frame(&mut self, ctx: &mut FrameContext)`: Runs once per rendered frame and only updates presentation, such as camera smoothing, animation, particles, and UI. `FrameContext` provides frame input, the frame's real `dt`, and the interpolation factor. It has no command queue and no access to other objects.
  - `draw(&self, gfx: &mut Draw)`: Read-only. Records draw commands. `Draw` carries the interpolation factor.
- The tick rate is configurable, defaulting to 60 Hz.
- The number of ticks per frame is capped. When the cap is hit, the game slows down instead of falling further behind.
- Deferred commands are applied after each tick.
- Each rendered frame runs in this order: accumulate real time, run zero or more ticks, run every object's `frame`, then run every object's `draw`.
- The engine keeps separate tick and frame input views, so "just pressed" fires exactly once in each.
- Each frame provides an interpolation factor between 0 and 1, the fraction of a tick that has elapsed since the last one, so games can blend between the previous and current state.
- The core exposes `tick()` to advance exactly one tick, used by headless runs and tests, and `advance(real_dt)` to accumulate real time, run the ticks that fit, and run frame callbacks, used by the runner.

## Consequences

- Game logic gives the same results at any frame rate, and headless runs and tests are exact: advancing one second at 60 Hz is 60 calls to `tick`.
- Physics and other simulation run at a stable, constant `dt`.
- Purpose-based names (`tick` changes the game, `frame` presents it) make it clearer where code belongs than Unity's `Update` and `FixedUpdate`.
- Spawning or modifying other objects from `frame` is a compile error. An object can still change its own gameplay fields in `frame`, which would make headless and windowed runs differ. The docs must state the rule clearly.
- Drawing can be skipped or repeated safely, which supports headless runs, split-screen, and screenshot tests.
- Objects moved in `tick` will judder on displays whose refresh rate doesn't match the tick rate unless they are interpolated. Games can interpolate using the interpolation factor. Whether the engine interpolates transforms automatically depends on whether it knows about object transforms, which is left to the object hierarchy ADR.
- Input arrives in a tick up to one tick late. Latency-sensitive input such as mouse look can be handled in `frame`.
- When ticks per frame hit the cap, game time runs slower than real time.
- The engine must maintain two input views and the bookkeeping for "just pressed" in each.
- [0004](0004-use-game-objects-in-scenes.md) and [0006](0006-split-engine-and-game.md) use `tick`, `frame`, `tick()`, and `advance(real_dt)` as defined here.
- Which platform requests, such as quitting from a pause menu, `FrameContext` may issue is left to implementation.

## Notes
