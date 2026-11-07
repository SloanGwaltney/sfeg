use std::any::{Any, TypeId};
use std::error::Error;
use std::fmt;

use crate::game_object::GameObject;
use crate::handle::{Handle, ObjectHandle};
use crate::math::Transform;
use crate::time::GameTime;

/// A collection of game objects and their transforms (ADR 0004).
///
/// Objects live in slots and are found by handle. Outside a tick, such as
/// while setting up the first scene, [`Scene::spawn`] adds an object right
/// away. During a tick, objects spawn and despawn through the
/// [`TickContext`], and those changes wait until the tick ends.
pub struct Scene {
    slots: Vec<Slot>,
    /// Free slot indices, reused last-in, first-out.
    free: Vec<usize>,
    commands: Commands,
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
}

#[derive(Default)]
struct Commands {
    spawns: Vec<PendingSpawn>,
    despawns: Vec<ObjectHandle>,
    /// How many new slots past the end the pending spawns have reserved.
    new_slots: usize,
}

struct PendingSpawn {
    handle: ObjectHandle,
    object: Box<dyn GameObject>,
    type_id: TypeId,
    local: Transform,
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Scene {
        Scene {
            slots: Vec::new(),
            free: Vec::new(),
            commands: Commands::default(),
        }
    }

    /// Adds `object` with the transform `local` right away.
    ///
    /// This is for building a scene outside a tick. During a tick, use
    /// [`TickContext::spawn`], which waits until the tick ends.
    pub fn spawn<T: GameObject>(&mut self, object: T, local: Transform) -> Handle<T> {
        let handle = self.reserve_now();
        self.insert(handle, Box::new(object), TypeId::of::<T>(), local);
        Handle::new(handle)
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

    /// Runs every object's tick, in slot order, then applies the spawns and
    /// despawns they asked for.
    pub(crate) fn tick(&mut self, game_time: &GameTime) {
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
                scene: self,
                self_handle,
            });

            // Despawns are deferred, so the slot still holds this object.
            self.entry_mut(self_handle)
                .expect("a ticking object's slot stays alive until the tick ends")
                .object = Some(object);
        }
        self.apply_commands();
    }

    fn apply_commands(&mut self) {
        // Taking the commands also resets the count of reserved new slots.
        let commands = std::mem::take(&mut self.commands);
        // Spawns first, so an object spawned and despawned in the same tick
        // ends up despawned.
        for spawn in commands.spawns {
            self.insert(spawn.handle, spawn.object, spawn.type_id, spawn.local);
        }
        for handle in commands.despawns {
            self.remove(handle);
        }
    }

    /// Picks a slot for an object that is added right away.
    fn reserve_now(&mut self) -> ObjectHandle {
        assert_eq!(
            self.commands.new_slots, 0,
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

    /// Picks a slot for an object that spawns when the tick ends. The slot
    /// stays empty until then, so the handle finds nothing.
    fn reserve_deferred(&mut self) -> ObjectHandle {
        if let Some(index) = self.free.pop() {
            let generation = self.slots[index].generation;
            return ObjectHandle { index, generation };
        }
        let index = self.slots.len() + self.commands.new_slots;
        self.commands.new_slots += 1;
        ObjectHandle {
            index,
            generation: 0,
        }
    }

    fn insert(
        &mut self,
        handle: ObjectHandle,
        object: Box<dyn GameObject>,
        type_id: TypeId,
        local: Transform,
    ) {
        let entry = Entry {
            object: Some(object),
            type_id,
            local,
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
    }

    /// Despawns the object `handle` refers to. Does nothing if it is gone.
    fn remove(&mut self, handle: ObjectHandle) {
        if self.entry(handle).is_none() {
            return;
        }
        let slot = &mut self.slots[handle.index];
        slot.entry = None;
        // A slot out of generations is retired instead of reused, so an old
        // handle can never match a new object.
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(handle.index);
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

impl Default for Scene {
    fn default() -> Scene {
        Scene::new()
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
/// transforms by handle, and queues spawns and despawns. Spawns and
/// despawns wait until the tick ends, after every object has ticked.
pub struct TickContext<'a> {
    game_time: &'a GameTime,
    scene: &'a mut Scene,
    self_handle: ObjectHandle,
}

impl TickContext<'_> {
    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }

    /// The handle of the object that is ticking.
    pub fn self_handle(&self) -> ObjectHandle {
        self.self_handle
    }

    /// This object's local transform.
    pub fn local(&self) -> &Transform {
        self.scene
            .local_of(self.self_handle)
            .expect("a ticking object's slot stays alive until the tick ends")
    }

    /// This object's local transform, to change it.
    pub fn local_mut(&mut self) -> &mut Transform {
        self.scene
            .local_of_mut(self.self_handle)
            .expect("a ticking object's slot stays alive until the tick ends")
    }

    /// Another object, or `None` if it is gone, hasn't spawned yet, or is
    /// the object that is ticking.
    pub fn get<T: GameObject>(&self, handle: Handle<T>) -> Option<&T> {
        self.scene.get(handle)
    }

    /// Another object, or `None` if it is gone, hasn't spawned yet, or is
    /// the object that is ticking.
    pub fn get_mut<T: GameObject>(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.scene.get_mut(handle)
    }

    /// Another object's local transform, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn local_of(&self, handle: impl Into<ObjectHandle>) -> Option<&Transform> {
        self.scene.local_of(handle)
    }

    /// Another object's local transform, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn local_of_mut(&mut self, handle: impl Into<ObjectHandle>) -> Option<&mut Transform> {
        self.scene.local_of_mut(handle)
    }

    /// Whether the object `handle` refers to is in the scene. An object
    /// despawned this tick is still alive until the tick ends.
    pub fn is_alive(&self, handle: impl Into<ObjectHandle>) -> bool {
        self.scene.is_alive(handle)
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
        self.scene.typed(handle)
    }

    /// Queues `object` to spawn with the transform `local` when the tick
    /// ends, and returns its handle now.
    ///
    /// Until the tick ends the handle finds nothing. The object's first
    /// tick is the next one.
    pub fn spawn<T: GameObject>(&mut self, object: T, local: Transform) -> Handle<T> {
        let handle = self.scene.reserve_deferred();
        self.scene.commands.spawns.push(PendingSpawn {
            handle,
            object: Box::new(object),
            type_id: TypeId::of::<T>(),
            local,
        });
        Handle::new(handle)
    }

    /// Queues the object `handle` refers to, which may be this one, to
    /// despawn when the tick ends.
    ///
    /// Until then it stays alive and still ticks if it hasn't yet. Nothing
    /// happens if it is already gone.
    pub fn despawn(&mut self, handle: impl Into<ObjectHandle>) {
        self.scene.commands.despawns.push(handle.into());
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
            scene.tick(&game_time);
            game_time.advance_one_tick();
        }
    }

    fn at(x: f32) -> Transform {
        Transform::from_position(Vector3::new(x, 0.0, 0.0)).unwrap()
    }

    #[test]
    fn objects_move_their_own_transform() {
        let mut scene = Scene::new();
        let walker = scene.spawn(Walker, at(10.0));
        run_ticks(&mut scene, 3);
        assert_eq!(
            scene.local_of(walker).unwrap().position(),
            Vector3::new(13.0, 0.0, 0.0)
        );
        assert!(scene.get(walker).is_some());
    }

    #[test]
    fn spawns_wait_until_the_tick_ends() {
        let mut scene = Scene::new();
        let spawner = scene.spawn(
            Probe::new(|ctx| {
                if ctx.game_time().tick_count() == 0 {
                    let child = ctx.spawn(Probe::new(|_| {}), at(1.0));
                    // The handle exists now, but the object doesn't yet.
                    assert!(!ctx.is_alive(child));
                    assert!(ctx.get(child).is_none());
                    assert!(ctx.local_of(child).is_none());
                }
            }),
            Transform::IDENTITY,
        );
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
        let mut scene = Scene::new();
        let quitter = scene.spawn(
            Probe::new(|ctx| {
                let me = ctx.self_handle();
                ctx.despawn(me);
                assert!(ctx.is_alive(me));
            }),
            Transform::IDENTITY,
        );
        let watcher = scene.spawn(
            Probe::new(|ctx| {
                // The quitter asked to despawn earlier this tick, but is
                // still here until the tick ends.
                let quitter = ObjectHandle {
                    index: 0,
                    generation: 0,
                };
                assert!(ctx.is_alive(quitter));
            }),
            Transform::IDENTITY,
        );
        run_ticks(&mut scene, 1);
        assert!(!scene.is_alive(quitter));
        assert!(scene.get(quitter).is_none());
        assert!(scene.local_of(quitter).is_none());
        assert_eq!(scene.get(watcher).unwrap().ticks, 1);
    }

    #[test]
    fn old_handles_never_find_a_reused_slot() {
        let mut scene = Scene::new();
        let first = scene.spawn(
            Probe::new(|ctx| {
                let me = ctx.self_handle();
                ctx.despawn(me);
            }),
            Transform::IDENTITY,
        );
        run_ticks(&mut scene, 1);
        let second = scene.spawn(Walker, Transform::IDENTITY);
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
        let mut scene = Scene::new();
        scene.spawn(
            Probe::new(|ctx| {
                if ctx.game_time().tick_count() == 0 {
                    let child = ctx.spawn(Walker, Transform::IDENTITY);
                    ctx.despawn(child);
                }
            }),
            Transform::IDENTITY,
        );
        run_ticks(&mut scene, 1);
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn deferred_spawns_fill_free_and_new_slots() {
        let mut scene = Scene::new();
        let doomed = scene.spawn(Walker, Transform::IDENTITY);
        scene.spawn(
            Probe::new(|ctx| match ctx.game_time().tick_count() {
                0 => ctx.despawn(ObjectHandle {
                    index: 0,
                    generation: 0,
                }),
                1 => {
                    // One reuses the freed slot, the other two are new.
                    let a = ctx.spawn(Walker, at(1.0));
                    let b = ctx.spawn(Walker, at(2.0));
                    let c = ctx.spawn(Walker, at(3.0));
                    assert_eq!([a, b, c].map(|h| h.untyped().index), [0, 2, 3]);
                }
                _ => {}
            }),
            Transform::IDENTITY,
        );
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

        let mut scene = Scene::new();
        let target = scene.spawn(Target { hits: 0 }, Transform::IDENTITY);
        scene.spawn(Shooter { target }, Transform::IDENTITY);
        run_ticks(&mut scene, 2);
        assert_eq!(scene.get(target).unwrap().hits, 2);
        assert_eq!(
            scene.local_of(target).unwrap().position(),
            Vector3::UNIT_Y * 2.0
        );
    }

    #[test]
    fn an_object_cant_find_itself_during_its_own_tick() {
        let mut scene = Scene::new();
        scene.spawn(
            Probe::new(|ctx| {
                let me = ctx.typed::<Probe>(ctx.self_handle()).unwrap();
                // Taken out of its slot, so no object, but the type check
                // and the transform still work.
                assert!(ctx.get(me).is_none());
                assert!(ctx.local_of(me).is_some());
            }),
            Transform::IDENTITY,
        );
        run_ticks(&mut scene, 1);
    }

    #[test]
    fn typed_checks_the_type() {
        let mut scene = Scene::new();
        let walker = scene.spawn(Walker, Transform::IDENTITY).untyped();
        assert!(scene.typed::<Walker>(walker).is_ok());
        assert_eq!(
            scene.typed::<Probe>(walker),
            Err(TypedHandleError::WrongType)
        );
    }

    #[test]
    fn despawning_a_gone_object_does_nothing() {
        let mut scene = Scene::new();
        scene.spawn(
            Probe::new(|ctx| {
                let me = ctx.self_handle();
                ctx.despawn(me);
                ctx.despawn(me);
            }),
            Transform::IDENTITY,
        );
        run_ticks(&mut scene, 1);
        assert!(scene.is_empty());
        // The slot was freed once, not twice.
        assert_eq!(scene.free, [0]);
    }

    #[test]
    fn out_of_generations_retires_the_slot() {
        let mut scene = Scene::new();
        let handle = scene.spawn(Walker, Transform::IDENTITY).untyped();
        scene.slots[0].generation = u32::MAX;
        let handle = ObjectHandle {
            generation: u32::MAX,
            ..handle
        };
        scene.remove(handle);
        assert!(scene.free.is_empty());
        let next = scene.spawn(Walker, Transform::IDENTITY);
        assert_eq!(next.untyped().index, 1);
    }

    #[test]
    fn handles_debug_without_the_scene() {
        let mut scene = Scene::new();
        let walker = scene.spawn(Walker, Transform::IDENTITY);
        assert_eq!(format!("{:?}", walker.untyped()), "ObjectHandle(0v0)");
        assert!(format!("{walker:?}").ends_with("Walker>(0v0)"));
    }
}
