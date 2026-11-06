# 0021. Scripting with Luau

**Date**: 2026-11-06

**Status**: Accepted

## Context

[0011](0011-no-editor-scenes-are-data.md) made modding opt-in per game and left behavior mods to optional embedded scripting, with the language left to a later ADR. Several later ADRs deferred mod questions here: namespacing for anything scripts add ([0015](0015-resource-loading.md)), mod material types ([0018](0018-surface-shaders-and-materials.md)), and mod collision layers ([0020](0020-physics-with-rapier.md)).

### Is the base game a mod?

An earlier plan was to build SFEG's own game as a mod, as much as possible, to prove the modding API. That pulls against 0011 and 0018, which keep the game's own data and materials compile-time checked. A better way to think about it: **the engine is to the game what the game is to a mod.** The engine is a library that makes building a game simple, and the game in turn decides what its mods can build. The base game stays Rust and keeps every compile-time check. Each game defines its own mod API, built partly from engine bindings and partly from its own types.

### Why not Rust mods

Native plugins (`.dll`, `.so`) were already ruled out in 0011. The deeper reason is control, not the audience. Mod authors are anyone who wants to extend a game. Once native code loads, it can do anything the process can, and there is no practical sandbox. An embedded language lets the engine and the game decide exactly what a mod can reach. Mods are hostile code, since players download them from anywhere, and malicious game mods have stolen real players' credentials.

### Language

The criteria, in order: control (sandboxable, no I/O unless granted, runaway scripts stoppable), a permissive license, static types, adequate speed and maturity, and fast editing and reloading.

- **Lua 5.4** (`mlua`): For these criteria it is Luau without the sandbox mode or the types. Sandboxing it by hand means stripping the standard library without stripping what scripts need, which is easy to get wrong.
- **WebAssembly** (`wasmtime`, `wasmi`): The strongest sandbox, and mods could be written in any language, including Rust. But mods must be compiled, which makes the edit loop slower and turns mods into opaque binaries that players and other modders can't read. There is also next to no learning material for WebAssembly outside the web, and almost none for game modding. Together with a compile toolchain, that is a rough start for newcomers.
- **JavaScript** (Boa, QuickJS), **Rune**, **Koto**: A heavy runtime whose sandbox wasn't designed for untrusted mods, or too niche.
- **Rhai**: Pure Rust (MIT/Apache) with a sizable community. It needs no FFI, so it calls Rust nearly directly, keeps the build pure Rust, and has richer number types. But its interpreter is slow, and it has no static types, so every mistake in a mod appears at runtime.
- **Luau** (`mlua`, MIT): Roblox's Lua dialect, built for untrusted code and used at very large scale. `mlua` supports its sandbox mode and interrupt callbacks. It has gradual static types, a command-line checker (`luau-analyze`), and a language server (`luau-lsp`) that loads definition files describing host APIs. Its costs: a C++ toolchain in the build, a C++ interpreter running untrusted code, a cost per call across the FFI boundary, and a single float number type.

Static types decide it. Luau's types are checked before running and erased at runtime, like TypeScript. Because a game's mod API is defined in Rust, its Luau definitions can be generated from the same code, so modders get type errors and autocomplete in their editor and the definitions can't drift from the runtime checks.

### How scripts plug in

[0012](0012-game-state-and-scene-state.md) put game state and scene state on the same `tick` and `frame` contract as objects. Every lifecycle in SFEG is a thing with hooks that run at a fixed point in the tick, so exposing any of them to mods uses the same mechanism: a trait of hooks, run on a different host.

Mods can then do only what the game exposes. That is the price of control. How far mods can go becomes the game's choice, from narrow content hooks, such as new enemies, to a generic scripted object and generic scripted state, which let mods build things the game never anticipated.

### Exposure and restriction

Toggling every individual function for mods is the ideal from a consumer's standpoint, not a builder's. The sandbox already provides safety, so the engine shouldn't create constant permission-flag contention. Exposure is per module. The engine also shouldn't restrict modders without a cause.

### Coroutines

Coroutines are mainly for suspending across time, such as `wait(1)` in the middle of a function. Suspending across ticks would let a hook call outlive its tick and its time budget. The remaining uses inside one call, such as custom iterators, are niche. It is better not to expose a runtime feature at all than to expose it unable to do its main job.

## Decision

### Scope

- **Scripting exists so games can offer behavior mods.** The engine never requires it, and modding stays opt-in per game (0011).
- **The base game is written in Rust.** SFEG does not expect base content to be scripted.
- **Each game defines its mod API explicitly**, from engine-provided bindings plus its own types. Nothing is exposed by default.
- **Mods can do only what the game exposes.** This is a deliberate trade for control. Each game chooses how broad that is.
- **SFEG's example game demonstrates both** narrow content hooks and the broad generic path, and ships example mods.
- **Native code mods stay ruled out.**

### Language

- **Mods are written in Luau**, embedded with `mlua`. Rhai was the runner-up.

### Hook interfaces

- **A game exposes a plug point as a Rust trait of hooks** marked with a macro such as `#[sfeg::mod_hooks]`. A mod script returns a table that implements it.
- **Required and optional hooks follow Rust's trait rule:**
  - No body: required.
  - An empty body `{}`: optional, and does nothing when the script omits it.
  - A body that returns a value: optional, with that value as the fallback.
  - A body returning `Option` with `None`: optional, meaning "no opinion," and the game falls back to its own logic.
- **Registered game object types are always Rust** ([0014](0014-scene-files-and-type-registration.md) is unchanged). A host type, such as a game's `ScriptedEnemy`, delegates to a script. The script is data, named with a namespaced name such as `ghost_mod:ghost`.
- **The engine ships a generic scripted object** whose hooks mirror `GameObject`, and **generic scripted game state and scene state hooks**, so a game can offer broad modding by opting in. Mods can then add things in the world and whole systems, such as a wave spawner.
- **Mods declare entry points in their manifest**, such as `behaviors: { ghost: (implements: "EnemyBehavior", script: "ghost.luau") }`. Scripts never call a registration function. The loader checks that each script exists, returns a table, and has every required hook. A failure is a mod load error.
- **Each instance gets its own Luau table (`self`)** for its state. Despawning the host object releases it.
- **Hooks follow `tick` and `frame` semantics** ([0008](0008-fixed-tick-and-per-frame-callbacks.md)). `ScriptTick` and `ScriptFrame` are different context types, and `ScriptFrame` has no spawning or physics writes. Because types are erased at runtime, the engine also rejects a tick-only call made from a frame hook. **There is no draw hook.** The host type draws from state the script sets.
- **Hook calls run synchronously inside the host's own hook**, so scripted objects run in the normal object order.

### Virtual machines

- **Each mod gets its own Luau VM.** Mods can't see each other's globals, and one mod can be stopped, disabled, or toggled without affecting others.
- **Mods can't call each other's functions.** A mod can use another mod's content through the game's API, such as spawning its enemy by name. Cross-mod communication is planned as an engine-mediated event structure, not as Rust relaying calls between VMs.

### API surface

- **Games expose engine bindings per module, with restricted variants**, such as `.expose(Physics::queries_only())` or `.expose(Audio::full())`. Modules cover transforms, spawning, physics, audio, input actions, UI, assets scoped to the mod (0015), and logging through `tracing`.
- **No globals.** Scripts reach the API only through the context passed to each hook.
- **Handles are opaque, typed userdata.** Each handle type, including each body kind from 0020, is its own declared class, so scripts can't forge a handle from a number.
- **Math types validate on construction**, per [0010](0010-wrap-glam-for-math.md). An invalid value is an error, never a silent correction.
- **Engine naming carries over unchanged**, such as `local_` and `world_` from [0013](0013-object-hierarchy-and-transforms.md).
- **Calls are coarse**, such as one spawn call with all parameters, to limit FFI crossings.
- **A mod reaches only objects it holds handles to**: its own instances, what it spawned, and what the game's API gives it. Handles from events and queries are untyped, and a mod can narrow them only to types the game exposes.
- **Mods may declare namespaced input actions** with default bindings in their manifest, such as `my_mod:toggle_map`. They appear in the player's rebinding screen and follow [0016](0016-input-actions.md)'s rules.

### Generated definitions

- **The game's build generates a `.d.luau` definition file** from everything it exposes, using the same macro that generates the runtime bindings. Doc comments carry over to hover text.
- Rust types map to Luau as follows: numbers to `number` (integers range-checked at the boundary), strings to `string`, `Option<T>` to `T?`, handles and math types to declared classes, unit-only enums to string literal unions, and data structs to table types.
- **Types are a development aid, not a guarantee.** Every call into Rust is checked at runtime regardless.

### Sandbox

- **Mods are treated as hostile.**
- **Luau's sandbox mode is on**: built-in libraries and globals are read-only, and each script has its own environment, so a mod can't tamper with shared state.
- **Standard libraries are an explicit allowlist**: `math`, `string`, `table`, `utf8`, `bit32`, `buffer`, and the clock functions from `os`. There is no `debug`, no `loadstring`, and no `coroutine`. Coroutines may return together with an explicit cross-tick suspension API, if one is designed.
- **`require` resolves only inside the mod's own files.**
- **There is no file system or network access.** Anything like that goes through the game's Rust API. Mod storage is left to the save games ADR.
- **Each hook call has a time budget**, enforced through Luau's interrupt callback, and **each VM has a memory limit**. The game sets both in its mod config TOML, with no defaults.
- **The sandbox is defense in depth, not a guarantee.** A memory-safety bug in Luau's C++ interpreter could let a mod escape. That bug would have to defeat a battle-tested runtime used by millions daily, which is far harder than a mod loading native code. SFEG's docs must not claim mods are safe.

### Errors

- **Expected failures across the boundary return typed tagged unions**, such as `{ ok: true, handle: EnemyHandle } | { ok: false, error: SpawnError }`. The type checker makes the modder check `ok` first, as Rust's `Result` does ([0009](0009-error-handling-and-logging.md)).
- **Misuse throws a Luau error**: a wrong argument type, a tick-only call from a frame hook, or an invalid math value.
- **The engine always catches errors that escape a hook.** A mod never crashes the game.
- **Hook calls return `Result<T, ModError>` on the Rust side.** The host type must handle a failed hook, and the compiler enforces it.
- **Exceeding the time budget or memory limit disables the whole mod.** The VM may be inconsistent, and failing loudly makes budgets easy to tune.
- **For other runtime errors, the game chooses a policy in its mod config TOML**, with no default: `disable_mod` or `continue`.
- **After a mod is disabled, its hooks return `Err(ModDisabled)`.** The engine logs the error and emits an event the game can poll to tell the player.
- **Error messages carry the mod name, file, line, and Luau stack trace.**

### Hot reload

- **A changed script module is reloaded in the mod's existing VM at a tick boundary.** Instance `self` tables keep their state, and the new functions are used from the next tick.
- **A failed reload keeps the old code** and logs the error, matching 0015.
- **Module-level locals are reset by a reload.** This is documented.
- **Manifest changes need the mod to restart.**
- **The engine doesn't type-check at reload.** It checks only that the code parses, returns a table, and has its required hooks.
- **Script hot reload works in release builds when the game opts in** with `script_hot_reload` in its mod config TOML, which is required with no default. This is separate from 0015's asset hot reload setting.

### Extending the engine from mods

- **Mods may declare namespaced collision layers** in their manifest, such as `ghost_mod:ethereal`. They are assigned from the game's free slots out of 0020's 32. The game sets `max_mod_layers` in its mod config TOML, which is required and may be 0. Running out of slots is a mod load error.
- **A mod may set interactions involving its own layers.**
- **A mod may change interactions between two game layers only if the game opts in** with `mods_may_edit_game_collisions`, required with no default. Conflicting edits follow 0015's load order: the later mod wins, and the override is logged.
- **A mod can't change interactions between two other mods' layers.** That waits for cross-mod events.
- **Mod material types will be implemented. Only the timing is open**, and one of the example game's demos will show them. The planned path: a mod supplies a WGSL surface function and a RON parameter schema, naga validates both at load, and scripts set parameters through a schema-checked API. Until then, mods use the game's materials with their own textures and values.
- **Custom asset loaders and UI primitives stay Rust-only.**

### Tooling

- **A game with modding enabled ships a mod kit**: the generated `game.d.luau` (only what the game exposes), a `luau-lsp` config template, an API reference generated from the same doc comments, and the example mods.
- **The game executable includes mod tools**, such as `game --mod-tools <command> <mod folder>`:
  - `types` generates the mod's own definitions from its manifest: its behaviors, actions, and layers as typed names.
  - `validate` runs every check the loader runs, using the same code, without starting the game.
  - `init` creates a new mod with a manifest, a starter script, and editor config.
- **SFEG doesn't bundle the Luau type checker.** Modders install `luau-lsp` or `luau-analyze` themselves.
- **Every mod manifest must declare the game version it was made against.** What happens on a mismatch is left to the versioning decision 0014 deferred.

### Mod config

The game's mod config TOML gathers the settings above, all required with no defaults:

```toml
hook_time_budget_ms = 2
memory_limit_mb = 64
on_script_error = "continue"
script_hot_reload = true
max_mod_layers = 8
mods_may_edit_game_collisions = false
```

## Consequences

- The base game keeps every compile-time check. Only mod code is dynamic.
- The mod API is explicit Rust in each game, and its Luau definitions are generated from it, so modders get editor type checking that matches the runtime.
- Mods can never exceed what the game exposes. A game that wants broad modding must opt into the generic scripted object and state hooks and expose broad bindings.
- One trait-of-hooks mechanism covers objects, game state, and scene state.
- Every escaped mod error lands in a `Result` the game must handle.
- Builds need a C++ toolchain, since Luau is C++.
- Luau has one float number type. Handles are opaque userdata, so this doesn't affect them.
- Each call across the FFI boundary has a cost, so the API favors coarse calls.
- Mods can't be written in Rust or any language other than Luau.
- Mods can't call each other's functions until cross-mod events are designed.
- Mods can't suspend across ticks. Timers and delays go through the API.
- Every game that enables modding must set time and memory budgets, an error policy, hot reload, layer slots, and whether mods may edit game collisions.
- Modders need nothing but the shipped game and an editor with `luau-lsp`.
- 0015's statement that hot reloading code is ruled out applies to Rust code, not mod scripts.

## Notes
