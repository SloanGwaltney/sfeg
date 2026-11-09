//! Parents, children, and world transforms (ADR 0013).

use std::error::Error;
use std::fmt;

use super::{Scene, TickContext};
use crate::handle::{Handle, ObjectHandle};
use crate::math::{Matrix4, Transform, Vector3};

/// Where an object sits in the hierarchy: at the root of the scene, or
/// under another object.
///
/// A handle converts into `Parent::Object`, so `parent` arguments take
/// either a handle or `Parent::Root`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parent {
    /// At the root of the scene. The local transform is the world
    /// transform.
    Root,
    /// Under this object. The local transform is relative to it.
    Object(ObjectHandle),
}

impl Parent {
    pub(super) fn handle(self) -> Option<ObjectHandle> {
        match self {
            Parent::Root => None,
            Parent::Object(handle) => Some(handle),
        }
    }

    fn from_handle(handle: Option<ObjectHandle>) -> Parent {
        handle.map_or(Parent::Root, Parent::Object)
    }
}

impl From<ObjectHandle> for Parent {
    fn from(handle: ObjectHandle) -> Parent {
        Parent::Object(handle)
    }
}

impl<T> From<Handle<T>> for Parent {
    fn from(handle: Handle<T>) -> Parent {
        Parent::Object(handle.untyped())
    }
}

/// Why a change to the hierarchy or a world transform could not be made.
///
/// A failed change leaves the scene as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HierarchyError {
    /// The object is gone, or hasn't spawned yet.
    Gone,
    /// The parent is gone, or hasn't spawned yet.
    ParentGone,
    /// The new parent is the object itself or one of its descendants.
    Cycle,
    /// A zero scale flattens space, so no local transform gives the
    /// requested world transform.
    ZeroScale,
    /// The local transform needed is too large to store as finite values,
    /// or the requested value wasn't finite.
    NotFinite,
}

impl fmt::Display for HierarchyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HierarchyError::Gone => write!(f, "the object is not in the scene"),
            HierarchyError::ParentGone => write!(f, "the parent is not in the scene"),
            HierarchyError::Cycle => {
                write!(f, "an object can't be a child of itself or its descendants")
            }
            HierarchyError::ZeroScale => write!(
                f,
                "a zero scale flattens space, so no local transform reaches that world transform"
            ),
            HierarchyError::NotFinite => write!(f, "the transform values must be finite"),
        }
    }
}

impl Error for HierarchyError {}

impl Scene {
    /// Where the object `handle` refers to sits, or `None` if it is gone.
    pub fn parent_of(&self, handle: impl Into<ObjectHandle>) -> Option<Parent> {
        Some(Parent::from_handle(self.entry(handle.into())?.parent))
    }

    /// The direct children of the object `handle` refers to, in the order
    /// they were added, or `None` if it is gone.
    pub fn children_of(&self, handle: impl Into<ObjectHandle>) -> Option<&[ObjectHandle]> {
        Some(&self.entry(handle.into())?.children)
    }

    /// The exact world matrix of the object `handle` refers to, or `None`
    /// if it is gone.
    ///
    /// It is computed now from the object's local transform and its
    /// parents', so it is never stale.
    pub fn world_matrix_of(&self, handle: impl Into<ObjectHandle>) -> Option<Matrix4> {
        let mut entry = self.entry(handle.into())?;
        let mut world = entry.local.to_matrix();
        while let Some(parent) = entry.parent {
            entry = self.entry(parent).expect("a parent outlives its children");
            world = world.then(entry.local.to_matrix());
        }
        Some(world)
    }

    /// The exact world position of the object `handle` refers to, or
    /// `None` if it is gone.
    pub fn world_position_of(&self, handle: impl Into<ObjectHandle>) -> Option<Vector3> {
        Some(self.world_matrix_of(handle)?.translation())
    }

    /// Moves the object `handle` refers to so its world position is
    /// `position`, by setting the local position that reaches it.
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::Gone`] if the object is gone.
    /// - [`HierarchyError::ZeroScale`] if a parent's zero scale makes the
    ///   position unreachable.
    /// - [`HierarchyError::NotFinite`] if `position`, or the local position
    ///   needed, isn't finite.
    pub fn set_world_position_of(
        &mut self,
        handle: impl Into<ObjectHandle>,
        position: Vector3,
    ) -> Result<(), HierarchyError> {
        let handle = handle.into();
        let parent = self.entry(handle).ok_or(HierarchyError::Gone)?.parent;
        if !position.is_finite() {
            return Err(HierarchyError::NotFinite);
        }
        let parent_world = self.parent_world_matrix(parent);
        let local_position = parent_world
            .inverse()
            .ok_or(HierarchyError::ZeroScale)?
            .transform_point(position);
        self.entry_mut(handle)
            .expect("checked above")
            .local
            .set_position(local_position)
            .map_err(|_| HierarchyError::NotFinite)
    }

    /// Moves the object `child` under `parent`, keeping its local
    /// transform, so it jumps to the same place relative to its new
    /// parent.
    ///
    /// Applies immediately, even during a tick. The object is added after
    /// the parent's other children. Interpolation skips the object until
    /// the next tick, so it doesn't slide to its new place (ADR 0013).
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::Gone`] if `child` is gone.
    /// - [`HierarchyError::ParentGone`] if `parent` is gone.
    /// - [`HierarchyError::Cycle`] if `parent` is `child` or one of its
    ///   descendants.
    pub fn set_parent_keep_local(
        &mut self,
        child: impl Into<ObjectHandle>,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        let child = child.into();
        let parent = parent.into().handle();
        self.check_new_parent(child, parent)?;
        self.link(child, parent);
        self.skip_interpolation(child);
        Ok(())
    }

    /// Moves the object `child` under `parent`, keeping where it is in the
    /// world by changing its local transform.
    ///
    /// Applies immediately, even during a tick. The object is added after
    /// the parent's other children. Its world position stays exact; its
    /// rotation and scale are approximate if the old or new parents have
    /// non-uniform scale (ADR 0013). Interpolation skips the object until
    /// the next tick, so it doesn't swing through a wrong place.
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::Gone`] if `child` is gone.
    /// - [`HierarchyError::ParentGone`] if `parent` is gone.
    /// - [`HierarchyError::Cycle`] if `parent` is `child` or one of its
    ///   descendants.
    /// - [`HierarchyError::ZeroScale`] if a zero scale in the child or
    ///   either set of parents makes its world transform impossible to
    ///   keep.
    /// - [`HierarchyError::NotFinite`] if the new local transform is too
    ///   large to store.
    pub fn set_parent_keep_world(
        &mut self,
        child: impl Into<ObjectHandle>,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        let child = child.into();
        let parent = parent.into().handle();
        self.check_new_parent(child, parent)?;
        let world = self.world_matrix_of(child).expect("checked above");
        let to_new_parent = self
            .parent_world_matrix(parent)
            .inverse()
            .ok_or(HierarchyError::ZeroScale)?;
        let local = to_transform(world.then(to_new_parent))?;
        self.link(child, parent);
        self.entry_mut(child).expect("checked above").local = local;
        self.skip_interpolation(child);
        Ok(())
    }

    fn check_new_parent(
        &self,
        child: ObjectHandle,
        parent: Option<ObjectHandle>,
    ) -> Result<(), HierarchyError> {
        if self.entry(child).is_none() {
            return Err(HierarchyError::Gone);
        }
        let mut ancestor = parent;
        while let Some(handle) = ancestor {
            if handle == child {
                return Err(HierarchyError::Cycle);
            }
            ancestor = self.entry(handle).ok_or(HierarchyError::ParentGone)?.parent;
        }
        Ok(())
    }

    /// The world matrix that a child of `parent` is relative to.
    fn parent_world_matrix(&self, parent: Option<ObjectHandle>) -> Matrix4 {
        match parent {
            Some(parent) => self
                .world_matrix_of(parent)
                .expect("a parent outlives its children"),
            None => Matrix4::IDENTITY,
        }
    }

    /// Moves `child` from its current parent to `parent`. Both must be
    /// alive, and the move must not make a cycle.
    pub(super) fn link(&mut self, child: ObjectHandle, parent: Option<ObjectHandle>) {
        let entry = self.entry_mut(child).expect("only live objects are linked");
        let old_parent = std::mem::replace(&mut entry.parent, parent);
        if let Some(old_parent) = old_parent {
            self.entry_mut(old_parent)
                .expect("a parent outlives its children")
                .children
                .retain(|&c| c != child);
        }
        if let Some(parent) = parent {
            self.entry_mut(parent)
                .expect("only live objects are linked")
                .children
                .push(child);
        }
    }

    /// Despawns the object and everything under it. Does nothing if it is
    /// gone.
    pub(super) fn remove_with_children(&mut self, handle: ObjectHandle) {
        if self.entry(handle).is_none() {
            return;
        }
        self.link(handle, None);
        let mut stack = vec![handle];
        while let Some(next) = stack.pop() {
            let entry = self.entry_mut(next).expect("children are alive");
            stack.append(&mut entry.children);
            self.free_slot(next.index);
        }
    }

    /// Moves the object's children to the root, keeping their world
    /// transforms, then despawns it. Does nothing if it is gone.
    pub(super) fn remove_keep_children(&mut self, handle: ObjectHandle) {
        let Some(entry) = self.entry(handle) else {
            return;
        };
        for child in entry.children.clone() {
            let world = self.world_matrix_of(child).expect("children are alive");
            match to_transform(world) {
                Ok(local) => {
                    self.link(child, None);
                    self.entry_mut(child).expect("children are alive").local = local;
                    self.skip_interpolation(child);
                }
                // Nobody is left to return an error to, so the child goes
                // with its parent.
                Err(error) => tracing::warn!(
                    ?child,
                    parent = ?handle,
                    %error,
                    "child despawned with its parent because its world transform can't be kept"
                ),
            }
        }
        self.remove_with_children(handle);
    }
}

fn to_transform(matrix: Matrix4) -> Result<Transform, HierarchyError> {
    if matrix.is_flat() {
        return Err(HierarchyError::ZeroScale);
    }
    matrix
        .to_transform_approx()
        .ok_or(HierarchyError::NotFinite)
}

const SELF_ALIVE: &str = "a ticking object's slot stays alive until the tick ends";

impl TickContext<'_> {
    /// Where this object sits in the hierarchy.
    pub fn parent(&self) -> Parent {
        self.current_scene
            .parent_of(self.self_handle)
            .expect(SELF_ALIVE)
    }

    /// This object's direct children, in the order they were added.
    pub fn children(&self) -> &[ObjectHandle] {
        self.current_scene
            .children_of(self.self_handle)
            .expect(SELF_ALIVE)
    }

    /// This object's exact world matrix. See [`Scene::world_matrix_of`].
    pub fn world_matrix(&self) -> Matrix4 {
        self.current_scene
            .world_matrix_of(self.self_handle)
            .expect(SELF_ALIVE)
    }

    /// This object's exact world position.
    pub fn world_position(&self) -> Vector3 {
        self.current_scene
            .world_position_of(self.self_handle)
            .expect(SELF_ALIVE)
    }

    /// Moves this object so its world position is `position`. See
    /// [`Scene::set_world_position_of`].
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::ZeroScale`] if a parent's zero scale makes the
    ///   position unreachable.
    /// - [`HierarchyError::NotFinite`] if `position`, or the local position
    ///   needed, isn't finite.
    pub fn set_world_position(&mut self, position: Vector3) -> Result<(), HierarchyError> {
        self.current_scene
            .set_world_position_of(self.self_handle, position)
    }

    /// Moves this object under `parent`, keeping its local transform. See
    /// [`Scene::set_parent_keep_local`].
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::ParentGone`] if `parent` is gone.
    /// - [`HierarchyError::Cycle`] if `parent` is this object or one of its
    ///   descendants.
    pub fn set_parent_keep_local(
        &mut self,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        self.current_scene
            .set_parent_keep_local(self.self_handle, parent)
    }

    /// Moves this object under `parent`, keeping where it is in the world.
    /// See [`Scene::set_parent_keep_world`].
    ///
    /// # Errors
    ///
    /// - [`HierarchyError::ParentGone`] if `parent` is gone.
    /// - [`HierarchyError::Cycle`] if `parent` is this object or one of its
    ///   descendants.
    /// - [`HierarchyError::ZeroScale`] if a zero scale makes the world
    ///   transform impossible to keep.
    /// - [`HierarchyError::NotFinite`] if the new local transform is too
    ///   large to store.
    pub fn set_parent_keep_world(
        &mut self,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        self.current_scene
            .set_parent_keep_world(self.self_handle, parent)
    }

    /// Where another object sits, or `None` if it is gone or hasn't spawned
    /// yet.
    pub fn parent_of(&self, handle: impl Into<ObjectHandle>) -> Option<Parent> {
        self.current_scene.parent_of(handle)
    }

    /// Another object's direct children, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn children_of(&self, handle: impl Into<ObjectHandle>) -> Option<&[ObjectHandle]> {
        self.current_scene.children_of(handle)
    }

    /// Another object's exact world matrix, or `None` if it is gone or
    /// hasn't spawned yet.
    pub fn world_matrix_of(&self, handle: impl Into<ObjectHandle>) -> Option<Matrix4> {
        self.current_scene.world_matrix_of(handle)
    }

    /// Another object's exact world position, or `None` if it is gone or
    /// hasn't spawned yet.
    pub fn world_position_of(&self, handle: impl Into<ObjectHandle>) -> Option<Vector3> {
        self.current_scene.world_position_of(handle)
    }

    /// Moves another object so its world position is `position`. See
    /// [`Scene::set_world_position_of`].
    ///
    /// # Errors
    ///
    /// The same as [`Scene::set_world_position_of`].
    pub fn set_world_position_of(
        &mut self,
        handle: impl Into<ObjectHandle>,
        position: Vector3,
    ) -> Result<(), HierarchyError> {
        self.current_scene.set_world_position_of(handle, position)
    }

    /// Moves another object under `parent`, keeping its local transform.
    ///
    /// # Errors
    ///
    /// The same as [`Scene::set_parent_keep_local`].
    pub fn set_parent_of_keep_local(
        &mut self,
        child: impl Into<ObjectHandle>,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        self.current_scene.set_parent_keep_local(child, parent)
    }

    /// Moves another object under `parent`, keeping where it is in the
    /// world.
    ///
    /// # Errors
    ///
    /// The same as [`Scene::set_parent_keep_world`].
    pub fn set_parent_of_keep_world(
        &mut self,
        child: impl Into<ObjectHandle>,
        parent: impl Into<Parent>,
    ) -> Result<(), HierarchyError> {
        self.current_scene.set_parent_keep_world(child, parent)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::game_object::GameObject;
    use crate::math::{Rotation, assert_close};
    use crate::time::{GameTime, TickRate};

    struct Thing;
    impl GameObject for Thing {}

    /// Runs a test-chosen action on its first tick.
    struct OnFirstTick(fn(&mut TickContext));
    impl GameObject for OnFirstTick {
        fn tick(&mut self, ctx: &mut TickContext) {
            if ctx.game_time().tick_count() == 0 {
                (self.0)(ctx);
            }
        }
    }

    fn tick(scene: &mut Scene) {
        scene.tick(
            &GameTime::new(TickRate::DEFAULT),
            &crate::input::InputView::empty(),
            &mut (),
            &mut crate::assets::AssetStore::new(std::path::PathBuf::new()),
        );
    }

    fn at(x: f32, y: f32) -> Transform {
        Transform::from_position(Vector3::new(x, y, 0.0)).unwrap()
    }

    fn scaled(scale: Vector3) -> Transform {
        Transform::new(Vector3::ZERO, Rotation::IDENTITY, scale).unwrap()
    }

    fn assert_vector_close(a: Vector3, b: Vector3) {
        assert_close(a.x(), b.x());
        assert_close(a.y(), b.y());
        assert_close(a.z(), b.z());
    }

    fn spawn(scene: &mut Scene, parent: impl Into<Parent>, local: Transform) -> ObjectHandle {
        scene
            .spawn_immediate(Thing, parent, local)
            .unwrap()
            .untyped()
    }

    /// A parent at (10, 0) turned a quarter turn, with a child one unit
    /// along its local +x.
    fn turned_parent_and_child(scene: &mut Scene) -> (ObjectHandle, ObjectHandle) {
        let turned = Transform::new(
            Vector3::new(10.0, 0.0, 0.0),
            Rotation::from_rotation_z(FRAC_PI_2).unwrap(),
            Vector3::ONE,
        )
        .unwrap();
        let parent = spawn(scene, Parent::Root, turned);
        let child = spawn(scene, parent, at(1.0, 0.0));
        (parent, child)
    }

    #[test]
    fn children_follow_their_parent() {
        let mut scene = Scene::new(());
        let (parent, child) = turned_parent_and_child(&mut scene);
        assert_vector_close(
            scene.world_position_of(child).unwrap(),
            Vector3::new(10.0, 1.0, 0.0),
        );
        // Moving the parent moves the child without touching it.
        scene
            .local_of_mut(parent)
            .unwrap()
            .set_position(Vector3::new(20.0, 0.0, 0.0))
            .unwrap();
        assert_vector_close(
            scene.world_position_of(child).unwrap(),
            Vector3::new(20.0, 1.0, 0.0),
        );
        assert_eq!(scene.local_of(child).unwrap().position().x(), 1.0);
        assert_eq!(scene.parent_of(child), Some(Parent::Object(parent)));
        assert_eq!(scene.parent_of(parent), Some(Parent::Root));
        assert_eq!(scene.children_of(parent), Some(&[child][..]));
    }

    #[test]
    fn spawn_immediate_under_a_gone_parent_fails() {
        let mut scene = Scene::new(());
        let parent = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        scene.remove_with_children(parent);
        assert_eq!(
            scene.spawn_immediate(Thing, parent, Transform::IDENTITY),
            Err(HierarchyError::ParentGone)
        );
        assert!(scene.is_empty());
    }

    #[test]
    fn deferred_spawn_under_a_parent_spawned_the_same_tick() {
        let mut scene = Scene::new(());
        scene
            .spawn_immediate(
                OnFirstTick(|ctx| {
                    let parent = ctx.spawn_deferred(Thing, Parent::Root, at(5.0, 0.0));
                    ctx.spawn_deferred(Thing, parent, at(1.0, 0.0));
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        tick(&mut scene);
        let parent = ObjectHandle {
            index: 1,
            generation: 0,
        };
        let child = ObjectHandle {
            index: 2,
            generation: 0,
        };
        assert_eq!(scene.parent_of(child), Some(Parent::Object(parent)));
        assert_vector_close(
            scene.world_position_of(child).unwrap(),
            Vector3::new(6.0, 0.0, 0.0),
        );
    }

    #[test]
    fn deferred_spawn_under_a_parent_gone_by_tick_end_fails() {
        let mut scene = Scene::new(());
        let doomed = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        let freed = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        scene.remove_with_children(freed);
        scene
            .spawn_immediate(
                OnFirstTick(|ctx| {
                    let doomed = ObjectHandle {
                        index: 0,
                        generation: 0,
                    };
                    ctx.despawn_deferred(doomed);
                    // The spawner took the freed slot 1, so both are new.
                    let a = ctx.spawn_deferred(Thing, doomed, Transform::IDENTITY);
                    let b = ctx.spawn_deferred(Thing, doomed, Transform::IDENTITY);
                    assert_eq!([a.untyped().index, b.untyped().index], [2, 3]);
                    // The parent is still alive, so this one spawns and is
                    // then despawned with it.
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        tick(&mut scene);
        assert!(!scene.is_alive(doomed));
        // Only the spawner is left.
        assert_eq!(scene.len(), 1);

        // Now spawn under a parent that is already gone.
        let mut scene = Scene::new(());
        let gone = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        scene.remove_with_children(gone);
        scene
            .spawn_immediate(
                OnFirstTick(|ctx| {
                    let gone = ObjectHandle {
                        index: 0,
                        generation: 0,
                    };
                    let a = ctx.spawn_deferred(Thing, gone, Transform::IDENTITY);
                    let b = ctx.spawn_deferred(Thing, gone, Transform::IDENTITY);
                    let c = ctx.spawn_deferred(Thing, Parent::Root, Transform::IDENTITY);
                    assert_eq!(
                        [a, b, c].map(|h| h.untyped().index),
                        [1, 2, 3],
                        "slot 0 went to the spawner"
                    );
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        tick(&mut scene);
        // The failed spawns' handles never find anything, and the root
        // spawn after them still lands in its reserved slot.
        assert!(!scene.is_alive(ObjectHandle {
            index: 1,
            generation: 0
        }));
        assert!(!scene.is_alive(ObjectHandle {
            index: 2,
            generation: 0
        }));
        assert!(scene.is_alive(ObjectHandle {
            index: 3,
            generation: 0
        }));
        assert_eq!(scene.len(), 2);
        // Their slots are free for reuse.
        let reused = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        assert_eq!(
            reused,
            ObjectHandle {
                index: 2,
                generation: 1
            }
        );
    }

    #[test]
    fn despawn_takes_the_whole_subtree() {
        let mut scene = Scene::new(());
        let root = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        let child = spawn(&mut scene, root, Transform::IDENTITY);
        let grandchild = spawn(&mut scene, child, Transform::IDENTITY);
        let sibling = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        scene.remove_with_children(child);
        assert!(scene.is_alive(root));
        assert!(!scene.is_alive(child));
        assert!(!scene.is_alive(grandchild));
        assert!(scene.is_alive(sibling));
        assert_eq!(scene.children_of(root), Some(&[][..]));
    }

    #[test]
    fn despawn_keep_children_moves_them_to_the_root_in_place() {
        let mut scene = Scene::new(());
        let (parent, child) = turned_parent_and_child(&mut scene);
        let grandchild = spawn(&mut scene, child, at(0.0, 2.0));
        let before = scene.world_position_of(grandchild).unwrap();
        scene.remove_keep_children(parent);
        assert!(!scene.is_alive(parent));
        assert_eq!(scene.parent_of(child), Some(Parent::Root));
        assert_vector_close(
            scene.local_of(child).unwrap().position(),
            Vector3::new(10.0, 1.0, 0.0),
        );
        // Grandchildren stay under their own parent and don't move.
        assert_eq!(scene.parent_of(grandchild), Some(Parent::Object(child)));
        assert_vector_close(scene.world_position_of(grandchild).unwrap(), before);
    }

    #[test]
    fn despawn_keep_children_drops_a_child_it_cant_keep() {
        let mut scene = Scene::new(());
        let flat = spawn(
            &mut scene,
            Parent::Root,
            scaled(Vector3::new(1.0, 0.0, 1.0)),
        );
        let child = spawn(&mut scene, flat, Transform::IDENTITY);
        scene.remove_keep_children(flat);
        assert!(!scene.is_alive(child));
        assert!(scene.is_empty());
    }

    #[test]
    fn cycles_are_errors_and_change_nothing() {
        let mut scene = Scene::new(());
        let root = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        let child = spawn(&mut scene, root, Transform::IDENTITY);
        let grandchild = spawn(&mut scene, child, Transform::IDENTITY);
        assert_eq!(
            scene.set_parent_keep_local(root, root),
            Err(HierarchyError::Cycle)
        );
        assert_eq!(
            scene.set_parent_keep_world(root, grandchild),
            Err(HierarchyError::Cycle)
        );
        assert_eq!(scene.parent_of(root), Some(Parent::Root));
        assert_eq!(scene.children_of(grandchild), Some(&[][..]));
    }

    #[test]
    fn reparenting_gone_objects_is_an_error() {
        let mut scene = Scene::new(());
        let a = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        let b = spawn(&mut scene, Parent::Root, Transform::IDENTITY);
        scene.remove_with_children(b);
        assert_eq!(
            scene.set_parent_keep_local(a, b),
            Err(HierarchyError::ParentGone)
        );
        assert_eq!(scene.set_parent_keep_world(b, a), Err(HierarchyError::Gone));
    }

    #[test]
    fn keep_local_jumps_and_keep_world_stays() {
        let mut scene = Scene::new(());
        let (parent, _) = turned_parent_and_child(&mut scene);
        let jumper = spawn(&mut scene, Parent::Root, at(1.0, 0.0));
        let stayer = spawn(&mut scene, Parent::Root, at(1.0, 0.0));

        scene.set_parent_keep_local(jumper, parent).unwrap();
        assert_vector_close(
            scene.world_position_of(jumper).unwrap(),
            Vector3::new(10.0, 1.0, 0.0),
        );

        scene.set_parent_keep_world(stayer, parent).unwrap();
        assert_vector_close(
            scene.world_position_of(stayer).unwrap(),
            Vector3::new(1.0, 0.0, 0.0),
        );
        // The parent is turned, so its rotation is undone in the local.
        let turned_back = scene
            .world_matrix_of(stayer)
            .unwrap()
            .transform_vector(Vector3::UNIT_X);
        assert_vector_close(turned_back, Vector3::UNIT_X);

        // Back to the root, still in place.
        scene.set_parent_keep_world(stayer, Parent::Root).unwrap();
        assert_vector_close(
            scene.local_of(stayer).unwrap().position(),
            Vector3::new(1.0, 0.0, 0.0),
        );
        assert_eq!(scene.children_of(parent).unwrap().len(), 2);
    }

    #[test]
    fn keep_world_into_a_flat_parent_fails() {
        let mut scene = Scene::new(());
        let flat = spawn(
            &mut scene,
            Parent::Root,
            scaled(Vector3::new(0.0, 1.0, 1.0)),
        );
        let object = spawn(&mut scene, Parent::Root, at(3.0, 0.0));
        assert_eq!(
            scene.set_parent_keep_world(object, flat),
            Err(HierarchyError::ZeroScale)
        );
        assert_eq!(scene.parent_of(object), Some(Parent::Root));
        assert_eq!(scene.local_of(object), Some(&at(3.0, 0.0)));
    }

    #[test]
    fn keep_world_position_is_exact_under_non_uniform_scale() {
        let mut scene = Scene::new(());
        let stretched = Transform::new(
            Vector3::new(1.0, 2.0, 0.0),
            Rotation::IDENTITY,
            Vector3::new(2.0, 1.0, 1.0),
        )
        .unwrap();
        let parent = spawn(&mut scene, Parent::Root, stretched);
        let turned = Transform::new(
            Vector3::new(5.0, 5.0, 0.0),
            Rotation::from_rotation_z(0.5).unwrap(),
            Vector3::ONE,
        )
        .unwrap();
        let object = spawn(&mut scene, Parent::Root, turned);
        scene.set_parent_keep_world(object, parent).unwrap();
        assert_vector_close(
            scene.world_position_of(object).unwrap(),
            Vector3::new(5.0, 5.0, 0.0),
        );
    }

    #[test]
    fn set_world_position_reaches_through_parents() {
        let mut scene = Scene::new(());
        let (_, child) = turned_parent_and_child(&mut scene);
        let target = Vector3::new(-3.0, 4.0, 1.0);
        scene.set_world_position_of(child, target).unwrap();
        assert_vector_close(scene.world_position_of(child).unwrap(), target);

        assert_eq!(
            scene.set_world_position_of(child, Vector3::new(f32::NAN, 0.0, 0.0)),
            Err(HierarchyError::NotFinite)
        );
        assert_vector_close(scene.world_position_of(child).unwrap(), target);

        let flat = spawn(&mut scene, Parent::Root, scaled(Vector3::ZERO));
        let stuck = spawn(&mut scene, flat, Transform::IDENTITY);
        assert_eq!(
            scene.set_world_position_of(stuck, Vector3::ONE),
            Err(HierarchyError::ZeroScale)
        );
    }

    #[test]
    fn reparenting_during_a_tick_applies_immediately() {
        let mut scene = Scene::new(());
        let parent = spawn(&mut scene, Parent::Root, at(10.0, 0.0));
        scene
            .spawn_immediate(
                OnFirstTick(|ctx| {
                    let parent = ObjectHandle {
                        index: 0,
                        generation: 0,
                    };
                    ctx.set_parent_keep_local(parent).unwrap();
                    assert_eq!(ctx.parent(), Parent::Object(parent));
                    assert_vector_close(ctx.world_position(), Vector3::new(11.0, 0.0, 0.0));
                    // An object can't become its own parent.
                    let me = ctx.self_handle();
                    assert_eq!(ctx.set_parent_keep_world(me), Err(HierarchyError::Cycle));
                    ctx.set_world_position(Vector3::new(12.0, 0.0, 0.0))
                        .unwrap();
                    assert_eq!(ctx.local().position().x(), 2.0);
                }),
                Parent::Root,
                at(1.0, 0.0),
            )
            .unwrap();
        tick(&mut scene);
        assert_eq!(scene.children_of(parent).unwrap().len(), 1);
    }
}
