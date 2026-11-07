use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Refers to a game object in a scene, whatever its type (ADR 0004).
///
/// A handle is a slot index plus a generation. When an object is despawned
/// its slot's generation goes up, so an old handle never finds the object
/// that later reuses the slot; lookups return `None` instead.
///
/// Handles are only made by the engine, by spawning. Use a typed
/// [`Handle`] where the type is known, and this one for mixed collections.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectHandle {
    pub(crate) index: usize,
    pub(crate) generation: u32,
}

impl fmt::Debug for ObjectHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectHandle({}v{})", self.index, self.generation)
    }
}

/// Refers to a game object of type `T` in a scene (ADR 0004).
///
/// Looking it up gives a `T` directly. Since a handle can only be made by
/// spawning a `T` or by checking an [`ObjectHandle`]'s type, a lookup can
/// only fail because the object is gone, never because the type is wrong.
pub struct Handle<T> {
    untyped: ObjectHandle,
    // `fn() -> T` keeps the handle `Copy`, `Send`, and `Sync` whatever `T` is.
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    pub(crate) fn new(untyped: ObjectHandle) -> Handle<T> {
        Handle {
            untyped,
            marker: PhantomData,
        }
    }

    /// The same handle without its type, for mixed collections.
    pub fn untyped(self) -> ObjectHandle {
        self.untyped
    }
}

impl<T> From<Handle<T>> for ObjectHandle {
    fn from(handle: Handle<T>) -> ObjectHandle {
        handle.untyped
    }
}

// Written by hand: derives would require `T` itself to be `Clone`, `Eq`,
// and so on, which a handle doesn't need.
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Handle<T> {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Handle<T>) -> bool {
        self.untyped == other.untyped
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.untyped.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handle<{}>({}v{})",
            std::any::type_name::<T>(),
            self.untyped.index,
            self.untyped.generation
        )
    }
}
