# Overview

**SFEG: Safe, Fun, Easy Games.**

This project is a simple game engine written in Rust, with a simple game built on top of it.

The goal is an engine that runs entirely safe Rust. That guarantee applies at the engine level, not to the libraries underneath it. At some layer, things like graphics need `unsafe` code, and that is acceptable in dependencies.

SFEG is built for smaller games and independent developers. Peak performance is not a goal, for two reasons:

1. Peak performance usually needs `unsafe` code.
2. The project puts safety, ease of use, and explicitness first.

If you need true peak performance in Rust, use [Bevy](https://bevyengine.org/) instead. It uses an ECS (Entity Component System) and is built explicitly for performance.

In SFEG, "easy" means explicit. The engine is easy to use because it is hard to misuse, not because it takes less code. When a shorter API would hide a decision or leave a footgun, SFEG picks the explicit one, even if it is more verbose.

SFEG is open source and uses only dependencies with permissive licenses (MIT, Apache-2.0, BSD, zlib, and similar). No proprietary licenses, and no copyleft licenses (GPL, LGPL, AGPL, MPL), so anyone can use SFEG without legal ambiguity. This rule covers code. Fonts may also use the SIL Open Font License (OFL), since it does not affect the software that uses them; SFEG never modifies or subsets a font automatically, because the OFL treats that as creating a modified version.

# Design Principles

## Same Data, Different Meaning

Some values have the same data but different meanings, such as points and vectors, local and world space, or meters and pixels. Separate types can stop them being mixed up, but they add conversions everywhere the values meet.

Types guard what the engine relies on. Names and docs guard what only the reader relies on.

- **Use separate types when mixing the values breaks a rule the engine relies on.** That is invalid state, such as an un-normalized rotation.
- **Otherwise, weigh the guard against its cost:**
  - How often is the mistake made?
  - How hard is it to spot when it is? Plausible-looking wrong answers are worse than crashes.
  - How much friction does the guard add to correct code?
- **If the guard doesn't pay for itself, share one type** and carry the meaning in method names and docs, such as `transform_point` and `transform_vector`.

A value that doesn't make sense is not automatically harmless. It may not break the engine, but it can still cause game bugs, which is why this is a judgment call and not a free pass. See [0023](docs/adr/0023-math-value-guarantees.md) for an example.

# Working with the User

## Skill Files

When creating skill files:

- Keep them short, sweet, and to the point.
- Keep the metadata extremely concise.
- Unless the user says otherwise, use exactly two sections:
  1. **Description**: a brief summary of what the skill does.
  2. **Steps**: the steps the skill takes.

## Naming

A name should say what something does, when it takes effect, and which one it means, without the reader needing the surrounding context.

- **Say when it takes effect.** If something can happen now or later, the name says which, such as `spawn_immediate` and `spawn_deferred`.
- **Say which one.** Qualify names that could mean more than one thing, such as `current_scene` (a game has several scenes) and `SceneCommands` (not just `Commands`, since GPU command buffers exist too).
- **Use plain verbs that state what happens to the data**, such as `set_previous_locals_to_current`, not vague ones like `remember`, `record`, or `sync`.
- **Document requirements the API imposes**, even when the types already enforce them, such as a core needing a first scene.
- **Give each error variant a one-line doc that describes when it happens**, such as "The object is gone, or hasn't spawned yet."
