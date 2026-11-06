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

# Working with the User

## Skill Files

When creating skill files:

- Keep them short, sweet, and to the point.
- Keep the metadata extremely concise.
- Unless the user says otherwise, use exactly two sections:
  1. **Description**: a brief summary of what the skill does.
  2. **Steps**: the steps the skill takes.
