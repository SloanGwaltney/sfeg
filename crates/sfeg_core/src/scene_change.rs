//! Changing scenes (ADR 0025).
//!
//! Besides the current scene, the core holds at most one next scene. The
//! game state or the scene state begins it, loads assets into its scope
//! and spawns objects into it while the current scene keeps running, then
//! switches to it or discards it. Switching and discarding wait until the
//! tick ends, so assets are only ever freed then.

use std::error::Error;
use std::fmt;

use crate::assets::AssetStore;
use crate::scene::Scene;

/// The next scene, and the switch or discard queued for it this tick.
pub(crate) struct SceneChange {
    next_scene: Option<Scene>,
    queued: Option<QueuedSceneChange>,
}

/// What happens to the next scene when the tick ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuedSceneChange {
    /// It becomes the current scene, and the current scene ends.
    SwitchToNextScene,
    /// It is dropped.
    DiscardNextScene,
}

impl SceneChange {
    pub(crate) fn new() -> SceneChange {
        SceneChange {
            next_scene: None,
            queued: None,
        }
    }

    /// Makes `scene` the next scene right away, and begins its scope.
    pub(crate) fn begin_next_scene(
        &mut self,
        scene: Scene,
        assets: &mut AssetStore,
    ) -> Result<(), BeginNextSceneError> {
        if self.next_scene.is_some() {
            return Err(BeginNextSceneError::NextSceneExists);
        }
        self.next_scene = Some(scene);
        assets.begin_next_scene_scope();
        Ok(())
    }

    pub(crate) fn next_scene(&self) -> Option<&Scene> {
        self.next_scene.as_ref()
    }

    pub(crate) fn next_scene_mut(&mut self) -> Option<&mut Scene> {
        self.next_scene.as_mut()
    }

    /// Queues `change` for when the tick ends.
    pub(crate) fn queue(&mut self, change: QueuedSceneChange) -> Result<(), QueueSceneChangeError> {
        if self.next_scene.is_none() {
            return Err(QueueSceneChangeError::NoNextScene);
        }
        if self.queued.is_some() {
            return Err(QueueSceneChangeError::ChangeAlreadyQueued);
        }
        self.queued = Some(change);
        Ok(())
    }

    /// Takes the queued change and the next scene it applies to, leaving
    /// neither. `None` if no change is queued.
    pub(crate) fn take_queued(&mut self) -> Option<(QueuedSceneChange, Scene)> {
        let change = self.queued.take()?;
        let next_scene = self
            .next_scene
            .take()
            .expect("a change is only queued while there is a next scene");
        Some((change, next_scene))
    }
}

/// Why a next scene couldn't begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BeginNextSceneError {
    /// A next scene already exists. Switch to it or discard it first. One
    /// with a switch or discard queued still exists until the tick ends.
    NextSceneExists,
}

impl fmt::Display for BeginNextSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeginNextSceneError::NextSceneExists => write!(
                f,
                "a next scene already exists; switch to it or discard it first"
            ),
        }
    }
}

impl Error for BeginNextSceneError {}

/// Why a switch to the next scene, or a discard of it, couldn't be queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueueSceneChangeError {
    /// There is no next scene: none was begun, or it was already switched
    /// to or discarded.
    NoNextScene,
    /// A switch or discard is already queued for when this tick ends.
    ChangeAlreadyQueued,
}

impl fmt::Display for QueueSceneChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueSceneChangeError::NoNextScene => write!(f, "there is no next scene"),
            QueueSceneChangeError::ChangeAlreadyQueued => write!(
                f,
                "a switch or discard is already queued for when this tick ends"
            ),
        }
    }
}

impl Error for QueueSceneChangeError {}

/// There is no next scene: none was begun, or it was already switched to
/// or discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoNextSceneError;

impl fmt::Display for NoNextSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "there is no next scene")
    }
}

impl Error for NoNextSceneError {}
