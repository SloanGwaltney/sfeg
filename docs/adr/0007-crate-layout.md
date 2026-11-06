# 0007. Crate Layout

**Date**: 2026-11-02

**Status**: Accepted

## Context

[0006](0006-split-engine-and-game.md) splits the engine into a window-free core and a platform layer, with winit and wgpu as the first platform. The core must never name winit, wgpu, or other platform types. This ADR decides how that split maps onto Rust crates.

The candidates considered were:

- **One engine crate with modules**: Simplest to set up. But nothing stops core code from importing wgpu or winit types, since the whole crate depends on them. Keeping the core abstract would rely on discipline alone. Over time that encourages core code written against vendor libraries and their types, which makes it hard to swap a vendor or support more than one. Separating them is easier now than later, once code depends on the shortcut.
- **One crate with feature flags**: The platform code sits behind a Cargo feature. This is still weak: whenever the feature is on, core code can reach platform types, and feature combinations are easy to get wrong.
- **Separate crates, plus a facade crate that re-exports them**: Each crate's `Cargo.toml` lists only the libraries it is allowed to use, so crossing a boundary is a compile error, not a code review comment. The game depends on the facade and doesn't need to know the engine is several crates.

### Splitting along vendor lines

The platform layer contains two separate concerns, each built on a different vendor library:

- **Windowing, input, and the loop**, built on winit.
- **Rendering**, built on wgpu. The renderer needs draw commands, assets, and a target. A wgpu surface can be created from anything that provides a raw window handle (the `raw-window-handle` traits), not from winit specifically, and a texture can also be a target.

Putting both in one platform crate would mix two vendors, the same problem the core/platform split avoids. Splitting them means moving to another windowing library, such as SDL, wouldn't touch the renderer, and changing the renderer wouldn't touch windowing. It also allows GPU rendering with no window at all: rendering to a texture and saving it enables screenshot tests, which sit between headless tests and running the full game.

Splitting further, such as separate audio, asset, or input crates, would be premature today. The rule for when a split stops being premature is when there is a vendor library actually in use whose concerns need separating from another vendor's. A hypothetical future vendor isn't enough. winit and wgpu meet that rule now.

### Other benefits

- The boundaries are checked by the compiler. [0001](0001-use-rust.md) puts weight on compile-time checks, partly because AI coding agents catch compiler errors right away. Crate boundaries turn "the core must not use platform types" and "the renderer must not use windowing types" into exactly that kind of check.
- The core builds and tests without compiling wgpu or winit, which keeps the test loop faster.
- Headless games and tools can depend on the core crate alone, with no windowing or GPU dependencies.

## Decision

SFEG will be a Cargo workspace with four engine crates:

- **`sfeg_core`**: The window-free core from 0006: scenes, game objects, the update context, deferred commands, time, the asset store, and draw commands. It doesn't depend on winit, wgpu, or any other platform library.
- **`sfeg_wgpu`**: The renderer. It takes draw commands and asset data and renders them to a target: a surface created from any raw window handle, or a texture. It depends on `sfeg_core` and wgpu, and not on winit.
- **`sfeg_winit`**: The runner. It owns the window, translates winit events into input data, and runs the windowed loop. Each frame it calls the core's `step` and `draw`, passes the draw commands to the renderer, presents the frame, and applies the platform requests the core produced. It depends on `sfeg_core`, `sfeg_wgpu`, and winit.
- **`sfeg`**: A facade that re-exports the other three, including `sfeg::run`. Games depend on `sfeg` only.

The interface the runner and renderer use from the core is small and fixed: input state, frame time, and window size go in; draw commands, platform requests, and asset data to upload or free come out. It lives in a dedicated module in `sfeg_core`, separate from the game-facing API.

The game built on top of SFEG lives in the same workspace as its own crate and depends only on `sfeg`. It exists to demonstrate the engine and may move out of the workspace later. Games that run headless may depend on `sfeg_core` directly.

Further crates are split out only when a vendor library actually in use needs its concerns separated from another vendor's.

## Consequences

- The compiler enforces that the core never uses platform types, and that the renderer never uses windowing types.
- Swapping the windowing library means replacing `sfeg_winit` without touching the renderer or the core. Swapping the renderer means replacing `sfeg_wgpu` without touching windowing or the core.
- The renderer can draw to a texture with no window, which enables screenshot tests.
- Games see one crate, `sfeg`, and don't need to know about the split.
- The core can be built and tested without compiling wgpu or winit.
- The interface the runner and renderer need from the core must be public in `sfeg_core`, even though games shouldn't use it. It is kept in a dedicated module and not re-exported from `sfeg`'s top level.
- Each crate is named after the vendor library it wraps, so the names stay accurate if more are added.
- There are four crates and three boundaries to maintain, and the facade's re-exports have to be kept up as the crates grow.
- The four crates are versioned and released together.

## Notes
