# 0001. Use Rust as the Programming Language

**Date**: 2026-11-01

**Status**: Accepted

## Context

SFEG is a simple game engine. It needs low-level control, and memory safety is one of the project's main pillars. The candidates considered were C, C++, C#, Go, Odin, Zig, and Rust.

- **C#**: Uses a garbage collector, which gives little control and would have to be engineered around. Its JIT is a poor fit for game development. Ahead-of-time (AOT) compilation avoids the JIT, but some C# features don't work with AOT, so you have to keep those limits in mind across the codebase.
- **Go**: Also garbage collected. If a garbage-collected language were chosen, Go would be the pick: its C-like structs make low-level work easier than in most languages. The garbage collector is still the reason to rule it out.
- **Odin**: Built for games, with the main game libraries vendored in. It would be the pick among the newer languages. But it is pre-1.0, which brings the risk of any new language, and it has no memory safety. Managing memory by hand goes against one of the project's main pillars.
- **Zig**: A new language with the same risk as Odin, and no reason to prefer it over Odin for games.
- **C**: Gives low-level control, but you manage memory by hand, which has historically been hard to get right. That goes against the project's focus on memory and code safety. Its build setup is also historically painful.
- **C++**: Has the same manual memory management problem. RAII and smart pointers help, but they still don't make memory easy to get right. For this project's use case, C++ offers nothing Rust doesn't already provide, and Rust has more modern conveniences and, arguably, better macros. C++ also shares C's painful build setup.

Rust gives low-level control and memory safety without a garbage collector. It is also the native language of wgpu, the graphics library (see [0002](0002-use-wgpu.md)).

## Decision

SFEG will be written in Rust.

## Consequences

- The engine can meet its goal of running entirely in safe Rust, with `unsafe` limited to dependencies.
- No garbage collector or JIT to engineer around.
- wgpu can be used in its native language, without bindings.
- The engine relies on the Rust ecosystem for windowing, audio, input, and other game libraries, rather than having them vendored in as Odin does.
- Contributors need to know Rust, including the borrow checker and its ownership rules.
- The borrow checker limits fine-grained control over memory. That helps safety but makes some performance optimizations harder.
- More checks can happen at compile time than in the other candidates. For example, a macro for a texture could make sure the texture file is part of the build.
- Compile-time checks help AI coding agents, which will write much of this codebase. Agents compile and test often, so an error caught by the compiler is one they find and fix right away. Moving more checks to compile time is a major advantage.
- There are far fewer game development tutorials and learning resources for Rust than for C and C++.
- Compile times are slow compared to Zig, Odin, and C#, though on par with C++. Cargo caches compiled dependencies and builds incrementally, so builds after the first are much faster. Adding or updating a dependency, changing its features, or changing the compiler version still triggers a rebuild of the affected crates. This downside is accepted for the advantages Rust brings.

## Notes
