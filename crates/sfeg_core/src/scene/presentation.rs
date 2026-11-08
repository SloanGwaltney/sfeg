//! Transform interpolation, the active camera, and drawing (ADR 0013,
//! 0024).

use super::{Entry, Scene, TickContext};
use crate::camera::{ActiveCamera, ActiveCameraError, Projection};
use crate::draw::DrawContext;
use crate::handle::ObjectHandle;
use crate::math::{Matrix4, Rotation, Transform};
use crate::platform::{CameraView, DrawList};

/// Whether drawing blends transforms between the last two ticks
/// (ADR 0013).
///
/// Interpolation keeps motion smooth when the display's refresh rate
/// doesn't match the tick rate, at the cost of showing the world up to one
/// tick behind. Games that want every tick shown exactly, such as
/// pixel-perfect 2D games, can turn it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformInterpolation {
    /// Blend each transform between the last two ticks. The default.
    #[default]
    Enabled,
    /// Draw each transform as the last tick left it.
    Disabled,
}

/// Which transforms a frame draws.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Blend {
    /// The transforms as the last tick left them.
    Current,
    /// Blended from the previous tick toward the current one by this
    /// factor, from 0 up to but not including 1.
    Between(f32),
}

impl Scene {
    /// Makes the object `camera` the active camera, looking through
    /// `projection` (ADR 0024).
    ///
    /// Any object can be the camera. The view follows its world position and
    /// rotation, interpolated like any other object, and ignores its scale.
    /// A scene has one active camera, so this replaces the one before.
    ///
    /// # Errors
    ///
    /// [`ActiveCameraError::Gone`] if `camera` is gone, or is a deferred
    /// spawn that hasn't been applied yet. The active camera is unchanged.
    pub fn set_active_camera(
        &mut self,
        camera: impl Into<ObjectHandle>,
        projection: impl Into<Projection>,
    ) -> Result<(), ActiveCameraError> {
        let camera = camera.into();
        if !self.is_alive(camera) {
            return Err(ActiveCameraError::Gone);
        }
        self.active_camera = Some(ActiveCamera::new(camera, projection.into()));
        self.missing_camera_reported = false;
        Ok(())
    }

    /// The active camera, or `None` if none was set or its object is gone.
    pub fn active_camera(&self) -> Option<ActiveCamera> {
        self.active_camera
            .filter(|camera| self.is_alive(camera.object()))
    }

    /// Sets every object's previous local transform to its current one, so
    /// drawing can blend from it. Runs before each tick.
    pub(super) fn set_previous_locals_to_current(&mut self) {
        for slot in &mut self.slots {
            if let Some(entry) = &mut slot.entry {
                entry.previous_local = entry.local;
            }
        }
    }

    /// Makes the object draw at its current local transform until the next
    /// tick, instead of sliding there from the previous one.
    pub(super) fn skip_interpolation(&mut self, handle: ObjectHandle) {
        if let Some(entry) = self.entry_mut(handle) {
            entry.previous_local = entry.local;
        }
    }

    /// Runs every object's draw and collects what they drew, seen from the
    /// active camera.
    ///
    /// Without an active camera nothing is drawn, and an error is logged
    /// once each time the scene loses its camera (ADR 0024).
    pub(crate) fn draw(&mut self, blend: Blend, interpolation_factor: f32) -> DrawList {
        let Some(camera) = self.active_camera() else {
            if !self.missing_camera_reported {
                self.missing_camera_reported = true;
                tracing::error!(
                    "the scene has no active camera, so nothing is drawn; set one with set_active_camera"
                );
            }
            return DrawList::without_camera();
        };
        let camera_object = camera.object();
        let camera_view = CameraView::new(
            self.blended_world_matrix(camera_object, blend)
                .expect("the active camera is alive")
                .translation(),
            self.blended_world_rotation(camera_object, blend)
                .expect("the active camera is alive"),
            camera.projection(),
        );

        let mut draw_commands = Vec::new();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(entry) = &slot.entry else {
                continue;
            };
            let handle = ObjectHandle {
                index,
                generation: slot.generation,
            };
            let world_matrix = self
                .blended_world_matrix(handle, blend)
                .expect("the object is alive");
            entry
                .object
                .as_ref()
                .expect("objects are only taken out during a tick")
                .draw(&mut DrawContext::new(
                    &mut draw_commands,
                    world_matrix,
                    interpolation_factor,
                ));
        }
        DrawList::new(camera_view, draw_commands)
    }

    /// The world matrix built from blended local transforms, so a child
    /// follows the arc of its parent's motion (ADR 0013).
    fn blended_world_matrix(&self, handle: ObjectHandle, blend: Blend) -> Option<Matrix4> {
        let mut entry = self.entry(handle)?;
        let mut world = blended_local(entry, blend).to_matrix();
        while let Some(parent) = entry.parent {
            entry = self.entry(parent).expect("a parent outlives its children");
            world = world.then(blended_local(entry, blend).to_matrix());
        }
        Some(world)
    }

    /// The world rotation built from blended local rotations alone,
    /// ignoring scale, so it exists even under a zero or mirrored scale.
    fn blended_world_rotation(&self, handle: ObjectHandle, blend: Blend) -> Option<Rotation> {
        let mut entry = self.entry(handle)?;
        let mut world = blended_local(entry, blend).rotation();
        while let Some(parent) = entry.parent {
            entry = self.entry(parent).expect("a parent outlives its children");
            world = world.then(blended_local(entry, blend).rotation());
        }
        Some(world)
    }
}

fn blended_local(entry: &Entry, blend: Blend) -> Transform {
    match blend {
        Blend::Current => entry.local,
        Blend::Between(factor) => {
            Transform::interpolate(&entry.previous_local, &entry.local, factor)
        }
    }
}

impl TickContext<'_> {
    /// Replaces this object's local transform without interpolating: until
    /// the next tick it draws at the new transform, instead of sliding there
    /// from the old one (ADR 0013).
    ///
    /// Use it for jumps, such as respawning at a checkpoint. For ordinary
    /// motion use [`TickContext::local_mut`], which interpolates.
    pub fn teleport_local(&mut self, local: Transform) {
        *self.local_mut() = local;
        self.current_scene.skip_interpolation(self.self_handle);
    }

    /// Makes the object `camera` the active camera. See
    /// [`Scene::set_active_camera`].
    ///
    /// # Errors
    ///
    /// [`ActiveCameraError::Gone`] if `camera` is gone, or was spawned this
    /// tick and won't exist until the tick ends.
    pub fn set_active_camera(
        &mut self,
        camera: impl Into<ObjectHandle>,
        projection: impl Into<Projection>,
    ) -> Result<(), ActiveCameraError> {
        self.current_scene.set_active_camera(camera, projection)
    }

    /// The active camera, or `None` if none was set or its object is gone.
    pub fn active_camera(&self) -> Option<ActiveCamera> {
        self.current_scene.active_camera()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Orthographic, Perspective};
    use crate::color::Color;
    use crate::game_object::GameObject;
    use crate::math::{Vector3, assert_close};
    use crate::platform::{BuiltinMesh, DrawCommand};
    use crate::scene::Parent;
    use crate::time::{GameTime, TickRate};

    /// Draws a cube and moves along +x by one unit each tick.
    struct Mover;

    impl GameObject for Mover {
        fn tick(&mut self, ctx: &mut TickContext) {
            let position = ctx.local().position() + Vector3::UNIT_X;
            ctx.local_mut().set_position(position).unwrap();
        }

        fn draw(&self, ctx: &mut DrawContext) {
            ctx.draw_cube(Color::WHITE);
        }
    }

    /// Draws a cube and does nothing else.
    struct Still;

    impl GameObject for Still {
        fn draw(&self, ctx: &mut DrawContext) {
            ctx.draw_cube(Color::BLACK);
        }
    }

    /// Runs a test-chosen action on each tick.
    struct Probe(fn(&mut TickContext));

    impl GameObject for Probe {
        fn tick(&mut self, ctx: &mut TickContext) {
            (self.0)(ctx);
        }
    }

    fn run_ticks(scene: &mut Scene, count: u64) {
        let mut game_time = GameTime::new(TickRate::DEFAULT);
        for _ in 0..count {
            scene.tick(&game_time, &crate::input::InputView::empty(), &mut ());
            game_time.advance_one_tick();
        }
    }

    fn projection() -> Projection {
        Perspective::new(1.0, 0.1, 100.0).unwrap().into()
    }

    /// A scene with a camera, so it draws.
    fn scene_with_camera() -> (Scene, ObjectHandle) {
        let mut scene = Scene::new(());
        let camera = scene
            .spawn_immediate(Probe(|_| {}), Parent::Root, Transform::IDENTITY)
            .unwrap()
            .untyped();
        scene.set_active_camera(camera, projection()).unwrap();
        (scene, camera)
    }

    fn at(x: f32) -> Transform {
        Transform::from_position(Vector3::new(x, 0.0, 0.0)).unwrap()
    }

    /// The x of each cube drawn, in draw order.
    fn drawn_xs(draw_list: &DrawList) -> Vec<f32> {
        draw_list
            .draw_commands()
            .iter()
            .map(|command| match command {
                DrawCommand::FlatMesh { world_matrix, .. } => world_matrix.translation().x(),
            })
            .collect()
    }

    #[test]
    fn draw_blends_between_the_last_two_ticks() {
        let (mut scene, _) = scene_with_camera();
        scene.spawn_immediate(Mover, Parent::Root, at(0.0)).unwrap();
        run_ticks(&mut scene, 2);
        // The last tick moved it from 1 to 2.
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.0), 0.0)), [1.0]);
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.25), 0.25)), [1.25]);
        assert_eq!(drawn_xs(&scene.draw(Blend::Current, 0.25)), [2.0]);
    }

    #[test]
    fn draw_commands_carry_the_mesh_and_color() {
        let (mut scene, _) = scene_with_camera();
        scene.spawn_immediate(Still, Parent::Root, at(3.0)).unwrap();
        let draw_list = scene.draw(Blend::Between(0.5), 0.5);
        assert_eq!(
            draw_list.draw_commands(),
            [DrawCommand::FlatMesh {
                mesh: BuiltinMesh::Cube,
                world_matrix: at(3.0).to_matrix(),
                color: Color::BLACK,
            }]
        );
    }

    #[test]
    fn a_new_object_does_not_slide_in() {
        let (mut scene, _) = scene_with_camera();
        scene
            .spawn_immediate(
                Probe(|ctx| {
                    if ctx.game_time().tick_count() == 0 {
                        ctx.spawn_deferred(Still, Parent::Root, at(5.0));
                    }
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.5), 0.5)), [5.0]);
    }

    #[test]
    fn a_teleport_does_not_slide() {
        let (mut scene, _) = scene_with_camera();
        struct Jumper;
        impl GameObject for Jumper {
            fn tick(&mut self, ctx: &mut TickContext) {
                if ctx.game_time().tick_count() == 1 {
                    ctx.teleport_local(at(100.0));
                }
            }
            fn draw(&self, ctx: &mut DrawContext) {
                ctx.draw_cube(Color::WHITE);
            }
        }
        scene
            .spawn_immediate(Jumper, Parent::Root, at(0.0))
            .unwrap();
        run_ticks(&mut scene, 2);
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.5), 0.5)), [100.0]);
        // The tick after, it interpolates again.
        run_ticks(&mut scene, 1);
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.5), 0.5)), [100.0]);
    }

    #[test]
    fn a_reparent_does_not_slide() {
        let (mut scene, _) = scene_with_camera();
        let far_parent = scene
            .spawn_immediate(Probe(|_| {}), Parent::Root, at(50.0))
            .unwrap();
        let child = scene.spawn_immediate(Mover, Parent::Root, at(0.0)).unwrap();
        run_ticks(&mut scene, 1);
        // Moved 0 -> 1 this tick, then jumps under the parent at 50.
        scene.set_parent_keep_local(child, far_parent).unwrap();
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.5), 0.5)), [51.0]);

        run_ticks(&mut scene, 1);
        scene.set_parent_keep_world(child, Parent::Root).unwrap();
        assert_eq!(drawn_xs(&scene.draw(Blend::Between(0.5), 0.5)), [52.0]);
    }

    #[test]
    fn children_follow_their_parents_blended_motion() {
        let (mut scene, _) = scene_with_camera();
        // A parent turning a quarter-turn per tick, with a child one unit
        // along its x axis.
        struct Turner;
        impl GameObject for Turner {
            fn tick(&mut self, ctx: &mut TickContext) {
                let quarter = Rotation::from_rotation_z(std::f32::consts::FRAC_PI_2).unwrap();
                let turned = ctx.local().rotation().then(quarter);
                ctx.local_mut().set_rotation(turned);
            }
        }
        let parent = scene
            .spawn_immediate(Turner, Parent::Root, Transform::IDENTITY)
            .unwrap();
        scene.spawn_immediate(Still, parent, at(1.0)).unwrap();
        run_ticks(&mut scene, 1);
        // Halfway through the turn, the child is on the arc, not on the
        // straight line between its ends.
        let draw_list = scene.draw(Blend::Between(0.5), 0.5);
        let DrawCommand::FlatMesh { world_matrix, .. } = draw_list.draw_commands()[0];
        let halfway = world_matrix.translation();
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        assert_close(halfway.x(), diagonal);
        assert_close(halfway.y(), diagonal);
    }

    #[test]
    fn the_camera_view_is_blended_too() {
        let mut scene = Scene::new(());
        let camera = scene.spawn_immediate(Mover, Parent::Root, at(0.0)).unwrap();
        scene.set_active_camera(camera, projection()).unwrap();
        run_ticks(&mut scene, 1);
        let draw_list = scene.draw(Blend::Between(0.5), 0.5);
        let view = draw_list.camera().unwrap().view_matrix();
        // The camera is halfway from 0 to 1, so the origin is half a unit
        // to its left.
        assert_close(view.transform_point(Vector3::ZERO).x(), -0.5);
    }

    #[test]
    fn the_camera_ignores_scale() {
        let mut scene = Scene::new(());
        let squashed = Transform::new(
            Vector3::new(0.0, 0.0, 10.0),
            Rotation::IDENTITY,
            Vector3::new(0.0, 3.0, -1.0),
        )
        .unwrap();
        let camera = scene
            .spawn_immediate(Probe(|_| {}), Parent::Root, squashed)
            .unwrap();
        let ortho = Orthographic::new(2.0, 0.0, 100.0).unwrap();
        scene.set_active_camera(camera, ortho).unwrap();
        let view = scene
            .draw(Blend::Current, 0.0)
            .camera()
            .unwrap()
            .view_matrix();
        let seen = view.transform_point(Vector3::new(1.0, 2.0, 0.0));
        assert_eq!(seen, Vector3::new(1.0, 2.0, -10.0));
    }

    #[test]
    fn no_camera_draws_nothing_and_reports_once_per_loss() {
        let mut scene = Scene::new(());
        scene.spawn_immediate(Still, Parent::Root, at(0.0)).unwrap();
        let draw_list = scene.draw(Blend::Current, 0.0);
        assert!(draw_list.camera().is_none());
        assert!(draw_list.draw_commands().is_empty());
        assert!(scene.missing_camera_reported);

        let camera = scene
            .spawn_immediate(
                Probe(|ctx| {
                    let me = ctx.self_handle();
                    ctx.despawn_deferred(me);
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        scene.set_active_camera(camera, projection()).unwrap();
        assert!(!scene.missing_camera_reported);
        assert_eq!(scene.draw(Blend::Current, 0.0).draw_commands().len(), 1);

        // The camera despawns itself, so the scene loses it.
        run_ticks(&mut scene, 1);
        assert!(scene.active_camera().is_none());
        assert!(scene.draw(Blend::Current, 0.0).draw_commands().is_empty());
        assert!(scene.missing_camera_reported);
    }

    #[test]
    fn a_gone_object_cant_be_the_camera() {
        let mut scene = Scene::new(());
        scene
            .spawn_immediate(
                Probe(|ctx| {
                    if ctx.game_time().tick_count() == 0 {
                        let later = ctx.spawn_deferred(Still, Parent::Root, Transform::IDENTITY);
                        assert_eq!(
                            ctx.set_active_camera(later, projection()),
                            Err(ActiveCameraError::Gone)
                        );
                    }
                }),
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        run_ticks(&mut scene, 1);
        assert!(scene.active_camera().is_none());
    }
}
