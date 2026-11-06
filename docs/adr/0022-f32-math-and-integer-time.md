# 0022. f32 Math and Integer Time

**Date**: 2026-11-06

**Status**: Accepted

## Context

[0010](0010-wrap-glam-for-math.md) wraps glam, which offers both f32 and f64 types, but didn't decide which precision SFEG's math types use. That blocks writing the math types.

### When f64 matters

Memory is not the deciding cost. A transform is about 40 bytes in f32 and 80 in f64, which is irrelevant at the scale of games SFEG targets.

The real case for f64 is large worlds. f32 has about 7 significant digits, so the gap between representable values grows with distance from the origin:

| Distance from origin | Smallest step in f32 |
|---|---|
| 1,000 units | about 0.00006 |
| 10,000 units | about 0.001 |
| 100,000 units | about 0.008 |
| 1,000,000 units | about 0.06 |

Far enough out, physics jitters and objects visibly shake, as in Kerbal Space Program's well-known "Kraken" bugs. A 2D game at 32 units per meter ([0020](0020-physics-with-rapier.md)) only reaches 100,000 units in a level about 3 km wide, where the step is still under a hundredth of a unit.

### Why f32

- **The GPU uses f32.** WGSL has no f64 in its core, so positions are converted to f32 for rendering regardless. Engines with large worlds handle them with camera-relative rendering or origin shifting, not f64 types alone.
- **The ecosystem defaults to f32**: wgpu, Rapier's default build, Taffy, kira, and glam's main types.
- **f64 is slower.** It halves SIMD throughput, and Rapier's f64 build is slower than its f32 build.
- **A precision switch is a trap.** A Cargo feature that changes types breaks Cargo's rule that features only add things: one dependency enabling it changes the types for every crate in the build. Making math types generic over precision adds complexity to every API.

### Time

Precision does matter for total elapsed time. Seconds stored as f32 degrade over a long session: after a day, the smallest step is about 8 ms, half a frame at 60 Hz. Ticks are already discrete ([0008](0008-fixed-tick-and-per-frame-callbacks.md)), so counting them is exact.

## Decision

- **SFEG's math types use f32.** There is no f64 option, either as a Cargo feature or as a generic parameter.
- **Game time is counted in ticks as a `u64`**, which is exact for any realistic session.
- **Elapsed seconds, where exposed, are f64**, derived from the tick count.
- **The per-tick `dt` and the per-frame `dt` are f32.** They are small values, so f32 is precise enough.
- **Large worlds are not a goal.** If they become one, they get their own ADR, likely using origin shifting or camera-relative rendering, not a precision swap.

## Consequences

- Math types match glam's main types, the GPU, and the f32 defaults of SFEG's dependencies, so values cross into wgpu, Rapier, Taffy, and kira without conversion.
- SFEG uses Rapier's f32 crates (`rapier2d`, `rapier3d`).
- Very large worlds will show precision jitter far from the origin. The docs should give a rough safe world size.
- Code that needs total time uses the tick count or the derived f64 seconds, never an f32 accumulator.

## Notes
