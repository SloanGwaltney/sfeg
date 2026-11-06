# 0023. Math Value Guarantees

**Date**: 2026-11-06

**Status**: Accepted

## Context

[0010](0010-wrap-glam-for-math.md) says rules about values are enforced by types, never by silent correction, but leaves open which math types carry guarantees, "such as rotations, directions, or points vs. vectors". Writing the first math types needs those answers.

### Non-finite values

An f32 can be NaN or infinite. Making non-finite vectors impossible looks like the obvious application of 0010, and checking a value on construction is cheap: a few comparisons.

The problem is that every arithmetic operation also constructs a new value. `a + b` returns a new `Vector3`; only `+=` changes one in place. A type rule like "always finite" has to hold for every value of the type, including the results of math, and ordinary math with finite inputs can produce non-finite results:

- `f32::MAX + f32::MAX` is infinity.
- Dividing by zero is infinity or NaN.
- Normalizing a zero vector is NaN.

Keeping the rule would mean every operator returns a `Result`, so `a + b * 2.0` could no longer be written plainly. A game engine does a great deal of math, and checking every operation would multiply the amount of code across the engine and every game for little benefit. Clamping or replacing bad results is ruled out by 0010, since that is silent correction.

The guarantee can still be kept where the engine actually relies on it: at the seams where values enter engine state, such as setting a transform or a physics body position. A non-finite value is rejected there with an error, at a clear place, without making every operator fallible.

### Points and vectors

As data, a point and a vector are identical: three f32s. They differ only in which operations make sense:

- point - point = vector, and point + vector = point.
- point + point, or scaling a point, has no meaning.
- Transforms translate points but not vectors. glam handles this with two methods, `transform_point3` and `transform_vector3`, not two types.

The difference is semantic. Marking a value as a point says that in this context certain operations don't make sense, and forbids them for that reason. But an operation that doesn't make sense is not an invalid state. The result is technically valid and probably won't break anything; it just raises the question of why it was done. That is a different kind of rule from "a rotation must be normalized", which the engine relies on for correct results.

Guarding it with separate types would cost conversions everywhere code mixes the two, even though the underlying data is the same.

### Unit directions

A `Direction3` type that guarantees a unit vector would be useful for raycasts and physics, but nothing needs it yet.

## Decision

- **Plain math types allow non-finite values.** Vectors and the arithmetic on them don't check for NaN or infinity, and operators don't return `Result`. This is a deliberate, general exception to making invalid state unrepresentable, limited to everyday arithmetic.
- **The engine rejects non-finite values at its seams.** Where engine state relies on finite values, such as transforms and physics, the API that takes the value checks it and returns an error. It never clamps or replaces the value.
- **Points and vectors share one type**, such as `Vector3`. Where the meaning changes the result, the method name carries it, such as `transform_point` vs. `transform_vector`.
- **Guarantee types are for rules the engine relies on**, such as a normalized `Rotation`, not for operations that merely don't make sense.
- **`Direction3` and other unit-vector types wait** until something needs them.

## Consequences

- Math reads like ordinary math: `a + b * 2.0` needs no error handling.
- A NaN can travel through a chain of game math and is only caught when it reaches an engine seam, so the error may point some distance from where the NaN was first produced.
- Every engine API that stores math values in engine state must validate them. Missing a seam lets a non-finite value into the engine, so seams need tests.
- Code can add two points or translate a direction without a compile error. Method names and docs carry the meaning instead of types.
- No conversions between point and vector types.
- The line between "invalid state" (guarded by a type) and "doesn't make sense" (not guarded) is the test for future guarantee types. It is written up as the "Same Data, Different Meaning" rubric in [AGENTS.md](../../AGENTS.md), which also weighs how often a mistake is made, how hard it is to spot, and how much friction a guard adds.

## Notes
