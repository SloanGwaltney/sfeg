//! Changing scenes (ADR 0025).
//!
//! Besides the current scene, the core holds at most one next scene. The
//! game state or the scene state begins it, loads assets into its scope
//! and spawns objects into it while the current scene keeps running, then
//! switches to it or discards it. Switching and discarding wait until the
//! tick ends, so assets are only ever freed then.
//!
//! A next scene can also come from a scene file (ADR 0026). It is built
//! when a tick ends after the file is read, and until then can't be
//! switched to.

use std::error::Error;
use std::fmt;

use crate::assets::{
    AssetManifest, AssetStore, ErrorChain, SceneFileAsset, SceneFileReadId, ScopeLoadCounts,
};
use crate::scene::Scene;
use crate::scene_file::{SceneFileError, SceneTypeRegistry, build_scene};

/// The next scene, and the switch or discard queued for it this tick.
pub(crate) struct SceneChange {
    next_scene: Option<NextScene>,
    queued: Option<QueuedSceneChange>,
}

/// Where the next scene is.
enum NextScene {
    /// Built and ready to switch to.
    Built {
        scene: Scene,
        /// Whether it was built from a scene file, which its load counts
        /// then include as ready.
        from_file: bool,
    },
    /// Its scene file is being read on a loader thread (ADR 0026).
    ReadingFile {
        asset: SceneFileAsset,
        read: SceneFileReadId,
    },
    /// Its scene file couldn't be read or built. The game discards it.
    FileFailed { error: SceneFileError },
}

/// What happens to the next scene when the tick ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuedSceneChange {
    /// It becomes the current scene, and the current scene ends.
    SwitchToNextScene,
    /// It is dropped.
    DiscardNextScene,
}

/// A queued change, taken when the tick ends.
pub(crate) enum SceneChangeToApply {
    /// Make this the current scene.
    SwitchTo(Scene),
    /// The next scene was dropped. End its scope.
    Discarded,
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
        self.next_scene = Some(NextScene::Built {
            scene,
            from_file: false,
        });
        assets.begin_next_scene_scope();
        Ok(())
    }

    /// Begins the next scene's scope and starts reading `asset`. The scene
    /// is built when a tick ends after the file is read (ADR 0026).
    pub(crate) fn begin_next_scene_from_file(
        &mut self,
        asset: SceneFileAsset,
        assets: &mut AssetStore,
    ) -> Result<(), BeginNextSceneError> {
        if self.next_scene.is_some() {
            return Err(BeginNextSceneError::NextSceneExists);
        }
        assets.begin_next_scene_scope();
        let read = assets.queue_scene_file_read(asset);
        self.next_scene = Some(NextScene::ReadingFile { asset, read });
        Ok(())
    }

    /// Whether there is a next scene, built or not.
    pub(crate) fn has_next_scene(&self) -> bool {
        self.next_scene.is_some()
    }

    /// The next scene, if it is built.
    pub(crate) fn next_scene(&self) -> Option<&Scene> {
        match &self.next_scene {
            Some(NextScene::Built { scene, .. }) => Some(scene),
            _ => None,
        }
    }

    /// The next scene, if it is built.
    pub(crate) fn next_scene_mut(&mut self) -> Option<&mut Scene> {
        match &mut self.next_scene {
            Some(NextScene::Built { scene, .. }) => Some(scene),
            _ => None,
        }
    }

    /// Why the next scene's file couldn't be read or built, if it failed.
    pub(crate) fn next_scene_file_error(&self) -> Option<&SceneFileError> {
        match &self.next_scene {
            Some(NextScene::FileFailed { error, .. }) => Some(error),
            _ => None,
        }
    }

    /// How the next scene's loads are going, counting its scene file
    /// with its textures.
    pub(crate) fn next_scene_scope_load_counts(
        &self,
        assets: &AssetStore,
    ) -> Result<ScopeLoadCounts, NoNextSceneError> {
        let next_scene = self.next_scene.as_ref().ok_or(NoNextSceneError)?;
        let scope = assets
            .next_scene_scope()
            .expect("the next scene's scope lasts as long as the next scene");
        let mut counts = assets.scope_load_counts(scope);
        match next_scene {
            NextScene::Built {
                from_file: false, ..
            } => {}
            NextScene::Built {
                from_file: true, ..
            } => counts.ready += 1,
            NextScene::ReadingFile { .. } => counts.loading += 1,
            NextScene::FileFailed { .. } => counts.failed += 1,
        }
        Ok(counts)
    }

    /// Queues `change` for when the tick ends.
    pub(crate) fn queue(&mut self, change: QueuedSceneChange) -> Result<(), QueueSceneChangeError> {
        let Some(next_scene) = &self.next_scene else {
            return Err(QueueSceneChangeError::NoNextScene);
        };
        if change == QueuedSceneChange::SwitchToNextScene
            && !matches!(next_scene, NextScene::Built { .. })
        {
            return Err(QueueSceneChangeError::NextSceneNotBuilt);
        }
        if self.queued.is_some() {
            return Err(QueueSceneChangeError::ChangeAlreadyQueued);
        }
        self.queued = Some(change);
        Ok(())
    }

    /// Takes the queued change, leaving no next scene. `None` if no change
    /// is queued.
    pub(crate) fn take_queued(&mut self) -> Option<SceneChangeToApply> {
        let change = self.queued.take()?;
        let next_scene = self
            .next_scene
            .take()
            .expect("a change is only queued while there is a next scene");
        Some(match (change, next_scene) {
            (QueuedSceneChange::SwitchToNextScene, NextScene::Built { scene, .. }) => {
                SceneChangeToApply::SwitchTo(scene)
            }
            (QueuedSceneChange::SwitchToNextScene, _) => {
                unreachable!("a switch is only queued to a built next scene")
            }
            // Dropping the next scene here drops its scene state and
            // objects, or forgets its file read.
            (QueuedSceneChange::DiscardNextScene, _) => SceneChangeToApply::Discarded,
        })
    }

    /// Builds the next scene if its file has been read, loading what the
    /// file refers to into the next scene's scope. Called when a tick
    /// ends, after any queued change was applied.
    pub(crate) fn build_next_scene_if_file_read(
        &mut self,
        scene_types: &SceneTypeRegistry,
        manifest: &AssetManifest,
        assets: &mut AssetStore,
    ) {
        let Some(NextScene::ReadingFile { asset, read }) = self.next_scene else {
            return;
        };
        let Some(read_result) = assets.take_finished_scene_file_read(read) else {
            return;
        };
        let scope = assets
            .next_scene_scope()
            .expect("the next scene's scope lasts as long as the next scene");
        let built =
            read_result.and_then(|tree| build_scene(tree, scene_types, manifest, assets, scope));
        self.next_scene = Some(match built {
            Ok(scene) => NextScene::Built {
                scene,
                from_file: true,
            },
            Err(error) => {
                tracing::error!(
                    scene_file = asset.name(),
                    error = %ErrorChain(&error),
                    "the next scene's file failed to load; the game should discard it"
                );
                NextScene::FileFailed { error }
            }
        });
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
    /// A switch was asked for while the next scene's file is still being
    /// read, or after it failed. Wait for it to build, or discard it.
    NextSceneNotBuilt,
}

impl fmt::Display for QueueSceneChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueSceneChangeError::NoNextScene => write!(f, "there is no next scene"),
            QueueSceneChangeError::ChangeAlreadyQueued => write!(
                f,
                "a switch or discard is already queued for when this tick ends"
            ),
            QueueSceneChangeError::NextSceneNotBuilt => write!(
                f,
                "the next scene isn't built yet: its file is still loading, or failed"
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
