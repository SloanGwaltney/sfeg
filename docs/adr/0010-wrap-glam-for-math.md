# 0010. Wrap glam for Math

**Date**: 2026-11-02

**Status**: Accepted

## Context

SFEG needs a math library for vectors, matrices, and quaternions. [0005](0005-normalize-around-3d.md) already assumes a 3D position, a quaternion rotation, and a 3D scale per transform.

### Candidates

- **glam**: Concrete, non-generic types (`Vec3`, `Quat`, `Mat4`, and f64 and integer versions). A simple API, fast compiles, readable errors, and the largest gamedev ecosystem (Bevy, macroquad, Avian). It supports `bytemuck` for GPU buffers. It is still 0.x, so breaking releases come often. Its types enforce no rules about their values, and it uses `unsafe` internally for SIMD, which is acceptable in dependencies.
- **nalgebra**: General linear algebra, generic over the number type. Its types carry guarantees, such as `UnitQuaternion` (always normalized), `Point3` vs. `Vector3`, and `Isometry3` (rotation and translation only). But heavy generics make signatures and error messages hard to read and compiles slower, which hurts ease of use.
- **cgmath**: The old default. Largely unmaintained.
- **ultraviolet**: Built around wide SIMD types for peak performance, which SFEG doesn't aim for. Smaller ecosystem.
- **vek**: Generic, with a small user base and nothing that stands out over glam.

### Type-level guarantees

A type-level guarantee is a rule about a value that the type itself enforces, so invalid values can't be built. For example, a quaternion only represents a rotation if its length is 1. In glam, `Quat::from_xyzw(1.0, 1.0, 0.0, 0.0)` compiles and runs, and rotating a vector by it also stretches the vector. Nothing reports the mistake.

Silently fixing such values, such as normalizing a rotation whenever it is set, was rejected. It hides a change to the data, which goes against SFEG putting explicitness first. Bad values should either be impossible to build or be rejected with an error, in line with [0009](0009-error-handling-and-logging.md).

### Re-exporting vs. wrapping

Re-exporting glam's types, even under new names with type aliases, still exposes glam's types. Every glam method comes with them, they mix freely with `glam` types, and the orphan rule prevents adding methods to them. Only wrapping each type in an SFEG-owned newtype with a private field gives SFEG control of the API.

Wrapping every type means forwarding constructors, methods, operators, and conversions. That is mostly boilerplate. The cost is ongoing, but this project is mostly AI-coded, which makes writing and maintaining boilerplate cheap. The exceptions are `bytemuck` and `serde` support, which need more care than forwarding.

## Decision

- **SFEG uses glam internally for its math.**
- **glam types are never exposed to games.** Every public math type is an SFEG-owned newtype that wraps a glam type with a private field, such as `pub struct Vector3(glam::Vec3)`.
- **SFEG owns the whole math API.** Methods, operators, and conversions exist only if SFEG adds them.
- **Rules about values are enforced by types, never by silent correction.** Where the engine relies on a rule, such as a rotation being normalized, a dedicated type enforces it. Its constructors either can't produce invalid values, return an error for invalid input, or name the correction explicitly, such as `from_quat_normalized`.
- **The math types implement `bytemuck` and `serde` support** so they can be uploaded to the GPU and serialized.

## Consequences

- Games learn one math API, SFEG's, and never see glam.
- glam can be upgraded, or replaced, without breaking games, as long as SFEG's API holds. A glam version bump only has to be checked against SFEG's own code.
- Every glam feature games need must be added to SFEG by hand. Until then, games can't use it through SFEG.
- Wrapping adds a large amount of forwarding code. A macro can generate much of it.
- Converting between SFEG types and other crates that use glam, such as physics libraries, needs explicit conversions.
- Invalid values like an unnormalized rotation can't reach engine APIs that take guarantee types.
- Which types get guarantees, such as rotations, directions, or points vs. vectors, is decided as the API is built. The transform types themselves belong to the object hierarchy ADR.

## Notes

- 2026-11-06: This ADR didn't decide between f32 and f64. [0022](0022-f32-math-and-integer-time.md) decides f32, with time counted in integer ticks.
