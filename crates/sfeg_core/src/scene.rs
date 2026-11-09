use std::any::{Any, TypeId};
use std::error::Error;
use std::fmt;

use crate::assets::{
    AssetScope, AssetStore, ReleaseTextureError, TextureAsset, TextureHandle, TextureState,
};
use crate::camera::ActiveCamera;
use crate::frame::FrameContext;
use crate::game_object::GameObject;
use crate::game_state::GameState;
use crate::handle::{Handle, ObjectHandle};
use crate::input::InputView;
use crate::math::Transform;
use crate::scene_change::SceneChange;
use crate::scene_state::{SceneState, SceneStateTickContext};
use crate::time::GameTime;

mod hierarchy;
mod presentation;

pub use hierarchy::{HierarchyError, Parent};
pub(crate) use presentation::Blend;
pub use presentation::TransformInterpolation;

/// A collection of game objects and their transforms (ADR 0004).
///
/// Objects live in slots and are found by handle. Outside a tick, such as
/// while setting up the first scene, [`Scene::spawn_immediate`] adds an
/// object right away. During a tick, objects spawn and despawn through the
/// [`TickContext`], and those changes wait until the tick ends.
///
/// Objects nest: each has a [`Parent`], and its transform is local to that
/// parent (ADR 0013). Moving a parent moves its children. Despawning a
/// parent despawns its children too.
///
/// Each scene has one scene state, created with the scene and dropped with
/// it (ADR 0012).
pub struct Scene {
    /// `None` only while the scene state runs its own tick.
    scene_state: Option<Box<dyn SceneState>>,
    slots: Vec<Slot>,
    /// Free slot indices, reused last-in, first-out.
    free: Vec<usize>,
    scene_commands: SceneCommands,
    /// The camera the game set. Its object may be gone since.
    active_camera: Option<ActiveCamera>,
    /// Whether the missing camera was already logged, so it is logged once
    /// each time the scene loses its camera, not every frame (ADR 0024).
    missing_camera_reported: bool,
}

struct Slot {
    generation: u32,
    /// `None` when the slot is free, or reserved for a spawn that hasn't
    /// been applied yet.
    entry: Option<Entry>,
}

struct Entry {
    /// `None` only while the object runs its own tick.
    object: Option<Box<dyn GameObject>>,
    type_id: TypeId,
    local: Transform,
    /// The local transform before the last tick, which drawing blends from
    /// (ADR 0013). Equal to `local` when interpolation is skipped.
    previous_local: Transform,
    /// Always a live object: despawning a parent despawns or detaches its
    /// children first.
    parent: Option<ObjectHandle>,
    /// In the order they were added.
    children: Vec<ObjectHandle>,
}

#[derive(Default)]
struct SceneCommands {
    spawns: Vec<PendingSpawn>,
    despawns: Vec<Despawn>,
    /// How many new slots past the end the pending spawns have reserved.
    new_slots: usize,
}

struct PendingSpawn {
    handle: ObjectHandle,
    object: Box<dyn GameObject>,
    type_id: TypeId,
    parent: Option<ObjectHandle>,
    local: Transform,
}

enum Despawn {
    WithChildren(ObjectHandle),
    KeepChildren(ObjectHandle),
}

impl Scene {
    /// An empty scene with `scene_state` as its scene state (ADR 0012). A
    /// scene without one passes `()`.
    pub fn new(scene_state: impl SceneState) -> Scene {
        Scene {
            scene_state: Some(Box::new(scene_state)),
            slots: Vec::new(),
            free: Vec::new(),
            scene_commands: SceneCommands::default(),
            active_camera: None,
            missing_camera_reported: false,
        }
    }

    /// Adds `object` under `parent`, with the transform `local` relative to
    /// it, right away.
    ///
    /// This is for building a scene outside a tick. During a tick, use
    /// [`TickContext::spawn_deferred`], which waits until the tick ends.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::ParentGone`] if `parent` is an object that is gone.
    /// Nothing is spawned.
    pub fn spawn_immediate<T: GameObject>(
        &mut self,
        object: T,
        parent: impl Into<Parent>,
        local: Transform,
    ) -> Result<Handle<T>, HierarchyError> {
        let parent = parent.into().handle();
        if let Some(parent) = parent
            && !self.is_alive(parent)
        {
            return Err(HierarchyError::ParentGone);
        }
        let handle = self.reserve_now();
        self.insert(handle, Box::new(object), TypeId::of::<T>(), parent, local);
        Ok(Handle::new(handle))
    }

    /// The scene state, or `None` if it isn't an `S`.
    pub fn scene_state<S: SceneState>(&self) -> Option<&S> {
        let scene_state: &dyn Any = self.scene_state.as_deref()?;
        scene_state.downcast_ref()
    }

    /// The scene state, to change it, or `None` if it isn't an `S`.
    pub fn scene_state_mut<S: SceneState>(&mut self) -> Option<&mut S> {
        let scene_state: &mut dyn Any = self.scene_state.as_deref_mut()?;
        scene_state.downcast_mut()
    }

    /// The object `handle` refers to, or `None` if it is gone.
    pub fn get<T: GameObject>(&self, handle: Handle<T>) -> Option<&T> {
        let object = self.entry(handle.untyped())?.object.as_deref()?;
        Some(downcast_ref(object))
    }

    /// The object `handle` refers to, or `None` if it is gone.
    pub fn get_mut<T: GameObject>(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let object = self.entry_mut(handle.untyped())?.object.as_deref_mut()?;
        Some(downcast_mut(object))
    }

    /// The local transform of the object `handle` refers to, or `None` if
    /// it is gone.
    pub fn local_of(&self, handle: impl Into<ObjectHandle>) -> Option<&Transform> {
        Some(&self.entry(handle.into())?.local)
    }

    /// The local transform of the object `handle` refers to, or `None` if
    /// it is gone.
    pub fn local_of_mut(&mut self, handle: impl Into<ObjectHandle>) -> Option<&mut Transform> {
        Some(&mut self.entry_mut(handle.into())?.local)
    }

    /// Whether the object `handle` refers to is in the scene.
    pub fn is_alive(&self, handle: impl Into<ObjectHandle>) -> bool {
        self.entry(handle.into()).is_some()
    }

    /// Checks that `handle` refers to a live `T` and returns a typed handle
    /// to it.
    ///
    /// # Errors
    ///
    /// - [`TypedHandleError::Gone`] if the object is gone.
    /// - [`TypedHandleError::WrongType`] if the object isn't a `T`.
    pub fn typed<T: GameObject>(
        &self,
        handle: ObjectHandle,
    ) -> Result<Handle<T>, TypedHandleError> {
        let entry = self.entry(handle).ok_or(TypedHandleError::Gone)?;
        if entry.type_id != TypeId::of::<T>() {
            return Err(TypedHandleError::WrongType);
        }
        Ok(Handle::new(handle))
    }

    /// How many objects are in the scene.
    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.entry.is_some())
            .count()
    }

    /// Whether the scene has no objects.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Runs the scene state's tick, then every object's in slot order, then
    /// applies the spawns and despawns they queued (ADR 0012).
    pub(crate) fn tick(
        &mut self,
        game_time: &GameTime,
        tick_input: &InputView,
        game_state: &mut dyn Any,
        assets: &mut AssetStore,
        scene_change: &mut SceneChange,
    ) {
        self.set_previous_locals_to_current();
        let mut scene_state = self
            .scene_state
            .take()
            .expect("the scene state is only taken out during its own tick");
        scene_state.tick(&mut SceneStateTickContext::new(
            game_time,
            tick_input,
            &mut *game_state,
            self,
            &mut *assets,
            scene_change,
        ));
        self.scene_state = Some(scene_state);
        // Spawns are deferred, so no slots are added while this runs.
        for index in 0..self.slots.len() {
            let slot = &mut self.slots[index];
            let generation = slot.generation;
            let Some(entry) = &mut slot.entry else {
                continue;
            };
            let mut object = entry
                .object
                .take()
                .expect("an object is only taken out during its own tick");
            let self_handle = ObjectHandle { index, generation };

            object.tick(&mut TickContext {
                game_time,
                tick_input,
                game_state: &mut *game_state,
                current_scene: self,
                self_handle,
                assets: &mut *assets,
            });

            // Despawns are deferred, so the slot still holds this object.
            self.entry_mut(self_handle)
                .expect("a ticking object's slot stays alive until the tick ends")
                .object = Some(object);
        }
        self.apply_commands();
    }

    /// Runs the scene state's frame, then every object's in slot order.
    pub(crate) fn frame(&mut self, ctx: &mut FrameContext) {
        self.scene_state
            .as_mut()
            .expect("the scene state is only taken out during a tick")
            .frame(ctx);
        for slot in &mut self.slots {
            if let Some(entry) = &mut slot.entry {
                entry
                    .object
                    .as_mut()
                    .expect("objects are only taken out during a tick")
                    .frame(ctx);
            }
        }
    }

    fn apply_commands(&mut self) {
        // Taking the scene commands also resets the count of reserved new slots.
        let scene_commands = std::mem::take(&mut self.scene_commands);
        // Spawns first, so an object spawned and despawned in the same tick
        // ends up despawned, and so do children spawned under a parent that
        // is despawned in the same tick.
        for spawn in scene_commands.spawns {
            if let Some(parent) = spawn.parent
                && !self.is_alive(parent)
            {
                // Nobody is left to return an error to (ADR 0013).
                tracing::warn!(
                    handle = ?spawn.handle,
                    ?parent,
                    "spawn failed because its parent was gone when the tick ended"
                );
                self.release(spawn.handle);
                continue;
            }
            self.insert(
                spawn.handle,
                spawn.object,
                spawn.type_id,
                spawn.parent,
                spawn.local,
            );
        }
        for despawn in scene_commands.despawns {
            match despawn {
                Despawn::WithChildren(handle) => self.remove_with_children(handle),
                Despawn::KeepChildren(handle) => self.remove_keep_children(handle),
            }
        }
    }

    /// Picks a slot for an object that is added right away.
    fn reserve_now(&mut self) -> ObjectHandle {
        assert_eq!(
            self.scene_commands.new_slots, 0,
            "slots are only reserved for later while a tick runs"
        );
        match self.free.pop() {
            Some(index) => ObjectHandle {
                index,
                generation: self.slots[index].generation,
            },
            None => ObjectHandle {
                index: self.slots.len(),
                generation: 0,
            },
        }
    }

    /// Queues `object` to spawn when the tick ends and returns its handle.
    pub(crate) fn queue_spawn<T: GameObject>(
        &mut self,
        object: T,
        parent: Parent,
        local: Transform,
    ) -> Handle<T> {
        let handle = self.reserve_deferred();
        self.scene_commands.spawns.push(PendingSpawn {
            handle,
            object: Box::new(object),
            type_id: TypeId::of::<T>(),
            parent: parent.handle(),
            local,
        });
        Handle::new(handle)
    }

    /// Queues `handle` to despawn with its children when the tick ends.
    pub(crate) fn queue_despawn_with_children(&mut self, handle: ObjectHandle) {
        self.scene_commands
            .despawns
            .push(Despawn::WithChildren(handle));
    }

    /// Queues `handle` to despawn when the tick ends, moving its children
    /// to the root first.
    pub(crate) fn queue_despawn_keep_children(&mut self, handle: ObjectHandle) {
        self.scene_commands
            .despawns
            .push(Despawn::KeepChildren(handle));
    }

    /// Picks a slot for an object that spawns when the tick ends. The slot
    /// stays empty until then, so the handle finds nothing.
    fn reserve_deferred(&mut self) -> ObjectHandle {
        if let Some(index) = self.free.pop() {
            let generation = self.slots[index].generation;
            return ObjectHandle { index, generation };
        }
        let index = self.slots.len() + self.scene_commands.new_slots;
        self.scene_commands.new_slots += 1;
        ObjectHandle {
            index,
            generation: 0,
        }
    }

    /// Fills a reserved slot. `parent` must be alive.
    fn insert(
        &mut self,
        handle: ObjectHandle,
        object: Box<dyn GameObject>,
        type_id: TypeId,
        parent: Option<ObjectHandle>,
        local: Transform,
    ) {
        let entry = Entry {
            object: Some(object),
            type_id,
            local,
            // A new object doesn't slide in from anywhere (ADR 0013).
            previous_local: local,
            parent: None,
            children: Vec::new(),
        };
        if handle.index == self.slots.len() {
            // New slots are reserved and inserted in the same order.
            self.slots.push(Slot {
                generation: handle.generation,
                entry: Some(entry),
            });
        } else {
            let slot = &mut self.slots[handle.index];
            assert!(
                slot.entry.is_none() && slot.generation == handle.generation,
                "a reserved slot stays empty until its spawn is applied"
            );
            slot.entry = Some(entry);
        }
        self.link(handle, parent);
    }

    /// Gives up a reserved slot whose spawn failed, so its handle never
    /// finds anything.
    fn release(&mut self, handle: ObjectHandle) {
        if handle.index == self.slots.len() {
            // New slots are reserved and released in the same order.
            self.slots.push(Slot {
                generation: handle.generation,
                entry: None,
            });
        }
        self.free_slot(handle.index);
    }

    /// Empties a slot and moves it to the next generation.
    fn free_slot(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        slot.entry = None;
        // A slot out of generations is retired instead of reused, so an old
        // handle can never match a new object.
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(index);
        }
    }

    fn entry(&self, handle: ObjectHandle) -> Option<&Entry> {
        let slot = self.slots.get(handle.index)?;
        if slot.generation != handle.generation {
            return None;
        }
        slot.entry.as_ref()
    }

    fn entry_mut(&mut self, handle: ObjectHandle) -> Option<&mut Entry> {
        let slot = self.slots.get_mut(handle.index)?;
        if slot.generation != handle.generation {
            return None;
        }
        slot.entry.as_mut()
    }
}

fn downcast_ref<T: GameObject>(object: &dyn GameObject) -> &T {
    let object: &dyn Any = object;
    object
        .downcast_ref()
        .expect("a typed handle only refers to objects of its type")
}

fn downcast_mut<T: GameObject>(object: &mut dyn GameObject) -> &mut T {
    let object: &mut dyn Any = object;
    object
        .downcast_mut()
        .expect("a typed handle only refers to objects of its type")
}

/// What a game object's tick can see and do (ADR 0004, 0008).
///
/// It reaches the object's own transform, other objects and their
/// transforms by handle, the game state and scene state by type
/// (ADR 0012), and queues spawns and despawns. Spawns and despawns wait
/// until the tick ends, after every object has ticked.
pub struct TickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
    game_state: &'a mut dyn Any,
    current_scene: &'a mut Scene,
    self_handle: ObjectHandle,
    assets: &'a mut AssetStore,
}

impl TickContext<'_> {
    /// Starts loading `asset` and holds it until the game ends. Returns its
    /// handle right away, while the file loads in the background.
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and the game holds it too.
    pub fn load_texture_into_game_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        self.assets.load_texture(asset, AssetScope::Game)
    }

    /// Starts loading `asset` and holds it until this scene ends. Returns
    /// its handle right away, while the file loads in the background.
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and this scene holds it too.
    pub fn load_texture_into_scene_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        let scope = self.assets.current_scene_scope();
        self.assets.load_texture(asset, scope)
    }

    /// Queues one of the game's loads of `texture` to be released when the
    /// tick ends (ADR 0025). Once every load by every scope is released or
    /// its scope has ended, the texture is freed.
    ///
    /// Release only what this object loaded ("load what you use").
    ///
    /// # Errors
    ///
    /// [`ReleaseTextureError::NotHeldByScope`] if the game has no load of
    /// it left to release, counting releases already queued.
    pub fn release_texture_from_game_scope_deferred(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), ReleaseTextureError> {
        self.assets.queue_release(texture, AssetScope::Game)
    }

    /// Queues one of this scene's loads of `texture` to be released when
    /// the tick ends (ADR 0025). Other objects' loads of it are kept, so
    /// an object can swap textures without breaking theirs.
    ///
    /// Release only what this object loaded ("load what you use").
    ///
    /// # Errors
    ///
    /// [`ReleaseTextureError::NotHeldByScope`] if this scene has no load of
    /// it left to release, counting releases already queued.
    pub fn release_texture_from_scene_scope_deferred(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), ReleaseTextureError> {
        let scope = self.assets.current_scene_scope();
        self.assets.queue_release(texture, scope)
    }

    /// Where `texture` is in loading.
    pub fn texture_state(&self, texture: TextureHandle) -> TextureState<'_> {
        self.assets.texture_state(texture)
    }

    /// Textures whose loads finished, successfully or not, before this
    /// tick started (ADR 0015). Each load is reported in exactly one tick.
    pub fn texture_loads_finished_before_this_tick(&self) -> &[TextureHandle] {
        self.assets.finished_this_tick()
    }

    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }

    /// The tick view of the input actions: "just pressed" means since the
    /// last tick (ADR 0008). Gameplay reads input here, so it runs the same
    /// in headless runs.
    pub fn tick_input(&self) -> &InputView {
        self.tick_input
    }

    /// The game state, or `None` if it isn't a `G` (ADR 0012).
    pub fn game_state<G: GameState>(&self) -> Option<&G> {
        self.game_state.downcast_ref()
    }

    /// The game state, to change it, or `None` if it isn't a `G`
    /// (ADR 0012).
    pub fn game_state_mut<G: GameState>(&mut self) -> Option<&mut G> {
        self.game_state.downcast_mut()
    }

    /// The current scene's scene state, or `None` if it isn't an `S`
    /// (ADR 0012).
    pub fn scene_state<S: SceneState>(&self) -> Option<&S> {
        self.current_scene.scene_state()
    }

    /// The current scene's scene state, to change it, or `None` if it isn't
    /// an `S` (ADR 0012).
    pub fn scene_state_mut<S: SceneState>(&mut self) -> Option<&mut S> {
        self.current_scene.scene_state_mut()
    }

    /// The handle of the object that is ticking.
    pub fn self_handle(&self) -> ObjectHandle {
        self.self_handle
    }

    /// This object's local transform.
    pub fn local(&self) -> &Transform {
        self.current_scene
            .local_of(self.self_handle)
            .expect("a ticking object's slot stays alive until the tick ends")
    }

    /// This object's local transform, to change it.
    ///
    /// The change is interpolated: drawing blends from the transform before
    /// this tick to the new one. For a jump, use
    /// [`TickContext::teleport_local`].
    pub fn local_mut(&mut self) -> &mut Transform {
        self.current_scene
            .local_of_mut(self.self_handle)
            .expect("a ticking object's slot stays alive until the tick ends")
    }

    /// Another object, or `None` if it is gone, hasn't spawned yet, or is
    /// the object that is ticking.
    pub fn get<T: GameObject>(&self, handle: Handle<T>) -> Option<&T> {
        self.current_scene.get(handle)
    }

    /// Another object, or `None` if it is gone, hasn't spawned yet, or is
    /// the object that is ticking.
    pub fn get_mut<T: GameObject>(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.current_scene.get_mut(handle)
    }

    /// Another object's local transform, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn local_of(&self, handle: impl Into<ObjectHandle>) -> Option<&Transform> {
        self.current_scene.local_of(handle)
    }

    /// Another object's local transform, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn local_of_mut(&mut self, handle: impl Into<ObjectHandle>) -> Option<&mut Transform> {
        self.current_scene.local_of_mut(handle)
    }

    /// Whether the object `handle` refers to is in the scene. An object
    /// despawned this tick is still alive until the tick ends.
    pub fn is_alive(&self, handle: impl Into<ObjectHandle>) -> bool {
        self.current_scene.is_alive(handle)
    }

    /// Checks that `handle` refers to a live `T`. See [`Scene::typed`].
    ///
    /// # Errors
    ///
    /// - [`TypedHandleError::Gone`] if the object is gone or hasn't spawned
    ///   yet.
    /// - [`TypedHandleError::WrongType`] if the object isn't a `T`.
    pub fn typed<T: GameObject>(
        &self,
        handle: ObjectHandle,
    ) -> Result<Handle<T>, TypedHandleError> {
        self.current_scene.typed(handle)
    }

    /// Queues `object` to spawn under `parent`, with the transform `local`
    /// relative to it, when the tick ends, and returns its handle now.
    ///
    /// Until the tick ends the handle finds nothing. The object's first
    /// tick is the next one. `parent` may be an object spawned earlier in
    /// this tick.
    ///
    /// If `parent` is gone when the tick ends, the object doesn't spawn,
    /// its handle never finds anything, and a warning is logged
    /// (ADR 0013).
    pub fn spawn_deferred<T: GameObject>(
        &mut self,
        object: T,
        parent: impl Into<Parent>,
        local: Transform,
    ) -> Handle<T> {
        self.current_scene.queue_spawn(object, parent.into(), local)
    }

    /// Queues the object `handle` refers to, which may be this one, to
    /// despawn with all of its children when the tick ends.
    ///
    /// Until then it stays alive and still ticks if it hasn't yet. Nothing
    /// happens if it is already gone. To keep the children, use
    /// [`TickContext::despawn_deferred_keep_children`].
    pub fn despawn_deferred(&mut self, handle: impl Into<ObjectHandle>) {
        self.current_scene
            .queue_despawn_with_children(handle.into());
    }

    /// Like [`TickContext::despawn_deferred`], but first moves the object's
    /// children to the root of the scene, keeping where they are in the
    /// world.
    ///
    /// Their new local transforms are their world transforms, which are
    /// approximate if a parent had non-uniform scale (ADR 0013). A child
    /// whose world transform can't be stored, such as one flattened by a
    /// zero scale, is despawned with the object and a warning is logged.
    pub fn despawn_deferred_keep_children(&mut self, handle: impl Into<ObjectHandle>) {
        self.current_scene
            .queue_despawn_keep_children(handle.into());
    }
}

/// Why [`Scene::typed`] could not give a typed handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypedHandleError {
    /// The object is gone, or hasn't spawned yet.
    Gone,
    /// The object is a different type.
    WrongType,
}

impl fmt::Display for TypedHandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypedHandleError::Gone => write!(f, "the object is not in the scene"),
            TypedHandleError::WrongType => write!(f, "the object is a different type"),
        }
    }
}

impl Error for TypedHandleError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vector3;
    use crate::time::TickRate;

    /// Moves one unit along +x each tick.
    struct Walker;

    impl GameObject for Walker {
        fn tick(&mut self, ctx: &mut TickContext) {
            let position = ctx.local().position() + Vector3::UNIT_X;
            ctx.local_mut().set_position(position).unwrap();
        }
    }

    /// Counts its ticks and runs a test-chosen action on each.
    struct Probe {
        ticks: u32,
        action: fn(&mut TickContext),
    }

    impl Probe {
        fn new(action: fn(&mut TickContext)) -> Probe {
            Probe { ticks: 0, action }
        }
    }

    impl GameObject for Probe {
        fn tick(&mut self, ctx: &mut TickContext) {
            self.ticks += 1;
            (self.action)(ctx);
        }
    }

    fn run_ticks(scene: &mut Scene, count: u64) {
        let mut game_time = GameTime::new(TickRate::DEFAULT);
        for _ in 0..count {
            scene.tick(
                &game_time,
                &InputView::empty(),
                &mut (),
                &mut crate::assets::AssetStore::new(std::path::PathBuf::new()),
                &mut crate::scene_change::SceneChange::new(),
            );
            game_time.advance_one_tick();
        }
    }

    fn at(x: f32) -> Transform {
        Transform::from_position(Vector3::new(x, 0.0, 0.0)).unwrap()
    }

    #[test]
    fn objects_move_their_own_transform() {
        let mut scene = Scene::new(());
        let walker = scene
            .spawn_immediate(Walker, Parent::Root, at(10.0))
            .unwrap();
        run_ticks(&mut scene, 3);
        assert_eq!(
            scene.local_of(walker).unwrap().position(),
            Vector3::new(13.0, 0.0, 0.0)
        );
        assert!(scene.get(walker).is_some());
    }

    #[test]
    fn spawns_wait_until_the_tick_ends() {
        let mut scene = Scene::new(());
        let spawner = scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    if ctx.game_time().tick_count() == 0 {
                        let child = ctx.spawn_deferred(Probe::new(|_| {}), Parent::Root, at(1.0));
                        // The handle exists now, but the object doesn't yet.
                        assert!(!ctx.is_alive(child));
                        assert!(ctx.get(child).is_none());
                        assert!(ctx.local_of(child).is_none());
                    }
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert_eq!(scene.len(), 2);

        run_ticks(&mut scene, 1);
        let child = scene
            .typed::<Probe>(ObjectHandle {
                index: 1,
                generation: 0,
            })
            .unwrap();
        // The child first ticks on the tick after it spawned.
        assert_eq!(scene.get(child).unwrap().ticks, 1);
        assert_eq!(scene.get(spawner).unwrap().ticks, 2);
    }

    #[test]
    fn despawned_objects_finish_the_tick() {
        let mut scene = Scene::new(());
        let quitter = scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    let me = ctx.self_handle();
                    ctx.despawn_deferred(me);
                    assert!(ctx.is_alive(me));
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        let watcher = scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    // The quitter asked to despawn earlier this tick, but is
                    // still here until the tick ends.
                    let quitter = ObjectHandle {
                        index: 0,
                        generation: 0,
                    };
                    assert!(ctx.is_alive(quitter));
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert!(!scene.is_alive(quitter));
        assert!(scene.get(quitter).is_none());
        assert!(scene.local_of(quitter).is_none());
        assert_eq!(scene.get(watcher).unwrap().ticks, 1);
    }

    #[test]
    fn old_handles_never_find_a_reused_slot() {
        let mut scene = Scene::new(());
        let first = scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    let me = ctx.self_handle();
                    ctx.despawn_deferred(me);
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        let second = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        // Same slot, new generation.
        assert_eq!(first.untyped().index, second.untyped().index);
        assert!(scene.get(first).is_none());
        assert!(!scene.is_alive(first));
        assert_eq!(
            scene.typed::<Walker>(first.untyped()),
            Err(TypedHandleError::Gone)
        );
        assert!(scene.get(second).is_some());
    }

    #[test]
    fn spawn_then_despawn_in_one_tick_ends_despawned() {
        let mut scene = Scene::new(());
        scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    if ctx.game_time().tick_count() == 0 {
                        let child = ctx.spawn_deferred(Walker, Parent::Root, Transform::IDENTITY);
                        ctx.despawn_deferred(child);
                    }
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn deferred_spawns_fill_free_and_new_slots() {
        let mut scene = Scene::new(());
        let doomed = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        scene
            .spawn_immediate(
                Probe::new(|ctx| match ctx.game_time().tick_count() {
                    0 => ctx.despawn_deferred(ObjectHandle {
                        index: 0,
                        generation: 0,
                    }),
                    1 => {
                        // One reuses the freed slot, the other two are new.
                        let a = ctx.spawn_deferred(Walker, Parent::Root, at(1.0));
                        let b = ctx.spawn_deferred(Walker, Parent::Root, at(2.0));
                        let c = ctx.spawn_deferred(Walker, Parent::Root, at(3.0));
                        assert_eq!([a, b, c].map(|h| h.untyped().index), [0, 2, 3]);
                    }
                    _ => {}
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 2);
        assert!(!scene.is_alive(doomed));
        assert_eq!(scene.len(), 4);
        for (index, x) in [(0, 1.0), (2, 2.0), (3, 3.0)] {
            let generation = if index == 0 { 1 } else { 0 };
            let handle = ObjectHandle { index, generation };
            assert_eq!(scene.local_of(handle).unwrap().position().x(), x);
        }
    }

    #[test]
    fn objects_reach_each_other_by_handle() {
        struct Target {
            hits: u32,
        }
        impl GameObject for Target {}

        struct Shooter {
            target: Handle<Target>,
        }
        impl GameObject for Shooter {
            fn tick(&mut self, ctx: &mut TickContext) {
                if let Some(target) = ctx.get_mut(self.target) {
                    target.hits += 1;
                }
                // Push the target along y.
                if let Some(local) = ctx.local_of_mut(self.target) {
                    let pushed = local.position() + Vector3::UNIT_Y;
                    local.set_position(pushed).unwrap();
                }
            }
        }

        let mut scene = Scene::new(());
        let target = scene
            .spawn_immediate(Target { hits: 0 }, Parent::Root, Transform::IDENTITY)
            .unwrap();
        scene
            .spawn_immediate(Shooter { target }, Parent::Root, Transform::IDENTITY)
            .unwrap();
        run_ticks(&mut scene, 2);
        assert_eq!(scene.get(target).unwrap().hits, 2);
        assert_eq!(
            scene.local_of(target).unwrap().position(),
            Vector3::UNIT_Y * 2.0
        );
    }

    #[test]
    fn an_object_cant_find_itself_during_its_own_tick() {
        let mut scene = Scene::new(());
        scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    let me = ctx.typed::<Probe>(ctx.self_handle()).unwrap();
                    // Taken out of its slot, so no object, but the type check
                    // and the transform still work.
                    assert!(ctx.get(me).is_none());
                    assert!(ctx.local_of(me).is_some());
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
    }

    #[test]
    fn typed_checks_the_type() {
        let mut scene = Scene::new(());
        let walker = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap()
            .untyped();
        assert!(scene.typed::<Walker>(walker).is_ok());
        assert_eq!(
            scene.typed::<Probe>(walker),
            Err(TypedHandleError::WrongType)
        );
    }

    #[test]
    fn despawning_a_gone_object_does_nothing() {
        let mut scene = Scene::new(());
        scene
            .spawn_immediate(
                Probe::new(|ctx| {
                    let me = ctx.self_handle();
                    ctx.despawn_deferred(me);
                    ctx.despawn_deferred(me);
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert!(scene.is_empty());
        // The slot was freed once, not twice.
        assert_eq!(scene.free, [0]);
    }

    #[test]
    fn out_of_generations_retires_the_slot() {
        let mut scene = Scene::new(());
        let handle = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap()
            .untyped();
        scene.slots[0].generation = u32::MAX;
        let handle = ObjectHandle {
            generation: u32::MAX,
            ..handle
        };
        scene.remove_with_children(handle);
        assert!(scene.free.is_empty());
        let next = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        assert_eq!(next.untyped().index, 1);
    }

    #[test]
    fn handles_debug_without_the_scene() {
        let mut scene = Scene::new(());
        let walker = scene
            .spawn_immediate(Walker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        assert_eq!(format!("{:?}", walker.untyped()), "ObjectHandle(0v0)");
        assert!(format!("{walker:?}").ends_with("Walker>(0v0)"));
    }
}
