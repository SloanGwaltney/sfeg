# 0018. Surface Shaders and Materials

**Date**: 2026-11-03

**Status**: Accepted

## Context

Games need custom shaders and materials. [0002](0002-use-wgpu.md) chose WGSL, [0005](0005-normalize-around-3d.md) has the renderer grow step by step toward 3D, and [0006](0006-split-engine-and-game.md) keeps built-in shaders in the platform layer, lets the core store WGSL without depending on wgpu, and leaves how custom shaders plug into the renderer to this ADR.

### Terms

- A **shader** is a program that runs on the GPU. Its vertex stage places geometry, and its fragment stage decides each pixel's color.
- A **material** is a shader plus the values it uses, such as textures, colors, and numbers.
- **Bind groups** hand values to a shader. The shader declares numbered slots, and the CPU side must fill them with matching types and memory layouts.
- A **pipeline** is a shader plus fixed settings, such as blending, depth writing, and face culling.

### Scope

wgpu and WGSL only know entry points, functions marked `@vertex`, `@fragment`, or `@compute`. If a game writes full entry points, it controls everything, but it must match the engine's internal layouts, handle transforms and the camera itself, and later do its own lighting. Every renderer change, such as adding lighting, would break it.

The alternative is an engine convention, often called a surface shader. Unity used the term, Godot's `fragment()` function setting values like `ALBEDO` works the same way, and Bevy lets materials supply fragment functions that call its lighting. The engine owns the real entry points. Before the game's code runs, it receives the vertex outputs, binds the camera, transform, and time, and builds the inputs. The game's function returns what the surface is, such as its color and alpha. After it, the engine applies lighting, fog, and transparency and writes the final color. The game's code is a plain function from inputs to outputs, so the engine can change everything around it, and new renderer features apply to existing materials without changes. The game gives up custom vertex effects unless it gets a vertex hook, a function the engine's vertex entry point calls in the same way.

This is the same split as transforms in [0013](0013-object-hierarchy-and-transforms.md): the engine owns what it coordinates across systems, and the game owns what is unique to it. Because the game's WGSL and the engine's WGSL are combined into one shader, errors must point back to the game's own files, or the split becomes a debugging problem.

### Declaring material data

The Rust side and the WGSL side must describe a material's data identically. WGSL's layout rules make this easy to get wrong by hand: a `vec3` takes 16 bytes, and structs get padding. A mismatch produces garbled values, not an error.

- **Setting values by name at runtime**, with the shader as the only definition, uses string names that fail at runtime, which [0016](0016-input-actions.md) rejected for input.
- **A Rust struct as the source of truth**, like Bevy's `AsBindGroup`, can only be checked against the shader at load time, since the struct exists only after the game compiles. It also needs a layout crate such as `encase` to get padding right.
- **The shader as the source of truth, with a Rust struct generated at build time**, makes a mismatch impossible, since one is generated from the other. Removing a parameter from the shader makes code that uses it fail to compile. The game has to write the shader anyway.

Build-time generation uses a build script, as the asset manifest does ([0015](0015-resource-loading.md)). Cargo runs `build.rs` before compiling the crate. It writes ordinary Rust into Cargo's output folder, the crate includes it, and Cargo reruns the script when shaders change. Mainstream crates such as `prost` generate code this way.

No WGSL parser needs to be written. naga, wgpu's shader compiler, parses WGSL and computes exact memory layouts, including padding. As of 2026-11-03:

- **naga_oil** (0.23.0, July 2026), maintained by the Bevy organization and used by every Bevy game, combines WGSL modules with `#import`.
- **wgsl_bindgen** (0.23.3, August 2026) and **wgsl_to_wgpu** (0.19.0, July 2026) generate Rust from WGSL. Both are active, but each has one maintainer and a small user base, and both generate wgpu types. 0006 keeps the core free of wgpu, and [0010](0010-wrap-glam-for-math.md) keeps vendor types away from games, so their output can't be used directly. They are useful references.

### Pipeline settings

WGSL has no syntax for blending, depth, or culling. They are set on the CPU side when a pipeline is created, so they need a home outside the shader. A different setting is a different pipeline, so changing one at runtime means building and caching another pipeline.

### Mods

Mods can't add generated Rust, so they can't add new material types through this contract. Modders likely won't write Rust anyway, so mods will need a different approach. Mod shaders are untrusted code running on the GPU. wgpu validates them, which catches problems such as out-of-bounds access, but a shader with an extremely long loop can still stall the GPU.

## Decision

### Scope

- **Game shaders are surface shaders**, an SFEG convention. The game writes a surface function, such as `fn surface(in: SurfaceInput) -> SurfaceOutput`, which runs inside the engine's fragment entry point.
- **A game may also write an optional vertex hook**, called inside the engine's vertex entry point to adjust positions.
- **The engine owns the shader entry points and machinery. The game owns its material's logic.**
- The surface shader contract, including `SurfaceInput`, `SurfaceOutput`, and the engine's WGSL helpers, is engine API with the same stability promise as the Rust API. Fields added later, such as for lighting, keep existing surface functions working.
- Fully custom pipelines are left to the raw-wgpu escape hatch deferred in 0006.

### Material data

- **The shader is the source of truth for a material's data.**
- **The build generates a Rust struct for each material** from its shader, using SFEG types such as `Vector3`, with padding taken from naga's layouts. Games never write material layouts by hand.
- **SFEG writes its own code generator** on top of naga, shipped as a library so a game's build script needs one call that handles both the asset manifest and materials.
- Shader modules are combined with a composer such as naga_oil.

### Engine data

- **Bind groups 0 and 1 are reserved for the engine**: group 0 for globals such as the camera and time, group 1 for per-object data such as the transform. **Group 2 is the game's material.**
- **Per-pixel values come through `SurfaceInput`**, such as UV, world position, and normal.
- **Other engine data and helpers come through explicit imports**, such as `#import sfeg::view`.
- **Built-in materials, such as sprites and shapes, use the same contract** as game materials.

### Pipeline settings

- **Pipeline settings, such as blending, depth writing, and culling, live in the material's data entry** next to its shader, so they hot reload and mods can override them.
- Pipeline settings can change at runtime. The engine builds each variant on first use and caches it.

### Checking

- **The build fails** on invalid WGSL, bindings outside group 2, or a missing or wrongly typed surface function or vertex hook.
- **Shader errors point to the game's own file and line.**
- The engine can write out the combined shader for debugging.

### Hot reload and mods

- Shader logic hot reloads. A reload that changes a material's parameter layout is rejected and the old version kept (see 0015), since the layout change needs a rebuild.
- Mods can override pipeline settings, and can replace a material's shader only with one that matches its layout, checked when the mod loads.
- **Mods can't add new material types.** A way for them to do so is left to a later ADR, likely alongside scripting.

## Consequences

- Most custom shaders are a few lines describing a surface, not full pipelines.
- New renderer features, such as lighting and fog, apply to existing materials without changes.
- Games can't replace the engine's entry points or do custom fragment-stage work outside the surface function until the escape hatch exists.
- SFEG must keep its WGSL contract stable, not only its Rust API.
- Rust and WGSL can't disagree about a material's data, and layout padding is never written by hand.
- Changing a material's parameters needs a rebuild. Changing its logic doesn't.
- SFEG maintains its own code generator, a small but real piece of work.
- Every SFEG game has a build script.
- Shader errors are caught at build time and reported in the game's own files.
- The first use of a new pipeline variant can cause a small hitch.
- Built-in materials exercise the same contract games use, so nothing the engine does with materials is impossible for a game.
- Mod shaders can stall the GPU, which the engine can't fully prevent. The docs must say so.

## Notes

- 2026-11-06: [0021](0021-scripting-with-luau.md) commits SFEG to letting mods add material types and records the planned path: a WGSL surface function and a RON parameter schema, validated by naga at load. Only the timing is open. Until then, this ADR's limit stands.
