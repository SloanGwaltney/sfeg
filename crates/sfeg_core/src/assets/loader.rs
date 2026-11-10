//! Background threads that read and decode asset files, so loading never
//! blocks the main thread (ADR 0015).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use super::png_decode::decode_png;
use super::{DecodedTexture, TextureHandle, TextureLoadError};
use crate::scene_file::{SceneFileError, SceneFileTree};

/// The most asset loader threads that run. Two keeps one large file from
/// holding up every other load, without competing much with the game for
/// CPU time.
///
/// For now all of them start. Later the loader may start fewer, based on
/// how many threads the CPU has (`std::thread::available_parallelism`),
/// but never more than this.
///
/// Asset loading has its own threads, not a pool shared with other engine
/// work: loads happen throughout the game and spend time blocked reading
/// files, so they must not hold up other work, nor wait behind it.
const MAX_ASSET_LOADER_THREADS: usize = 2;

/// One file for a loader thread to read and decode.
pub(super) enum LoadJob {
    Texture {
        texture: TextureHandle,
        path: PathBuf,
    },
    /// Reads and parses a scene file. Building it needs the game's types,
    /// so that happens on the main thread.
    SceneFile {
        read: SceneFileReadId,
        path: PathBuf,
    },
}

/// Tells one scene file read from another, so a read for a next scene
/// that was discarded can't be mistaken for a later one. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SceneFileReadId(pub(super) u64);

/// What a loader thread produced for one job.
pub(super) enum LoadResult {
    Texture {
        texture: TextureHandle,
        result: Result<DecodedTexture, TextureLoadError>,
    },
    SceneFile {
        read: SceneFileReadId,
        result: Result<SceneFileTree, SceneFileError>,
    },
}

/// The loader threads, and the channels to and from them.
///
/// The threads stop on their own once this is dropped, after finishing the
/// job they are on.
pub(super) struct AssetLoader {
    jobs: Sender<LoadJob>,
    results: Receiver<LoadResult>,
}

impl AssetLoader {
    /// Starts the loader threads.
    ///
    /// # Errors
    ///
    /// The operating system's error if no thread could start. If only
    /// some could, the loader runs with fewer.
    pub(super) fn start() -> Result<AssetLoader, std::io::Error> {
        let (jobs, job_receiver) = mpsc::channel::<LoadJob>();
        let (result_sender, results) = mpsc::channel();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let mut started = 0;
        let mut last_error = None;
        for index in 0..MAX_ASSET_LOADER_THREADS {
            let job_receiver = Arc::clone(&job_receiver);
            let result_sender = result_sender.clone();
            match std::thread::Builder::new()
                .name(format!("sfeg asset loader {index}"))
                .spawn(move || run_loader_thread(&job_receiver, &result_sender))
            {
                Ok(_) => started += 1,
                Err(error) => {
                    tracing::warn!(%error, "an asset loader thread couldn't start");
                    last_error = Some(error);
                }
            }
        }
        match (started, last_error) {
            (0, Some(error)) => Err(error),
            _ => Ok(AssetLoader { jobs, results }),
        }
    }

    /// Queues `job` for the next free loader thread.
    ///
    /// # Errors
    ///
    /// Gives the job back if every loader thread has stopped, which only
    /// happens if they all panicked outside a decoder.
    pub(super) fn queue(&self, job: LoadJob) -> Result<(), LoadJob> {
        self.jobs.send(job).map_err(|error| error.0)
    }

    /// A finished load, if one is waiting. Never blocks.
    ///
    /// `Err(Disconnected)` means every loader thread has stopped.
    pub(super) fn try_take_result(&self) -> Result<LoadResult, TryRecvError> {
        self.results.try_recv()
    }

    /// Blocks until a load finishes.
    ///
    /// # Errors
    ///
    /// If every loader thread has stopped, so nothing will ever finish.
    pub(super) fn wait_for_result(&self) -> Result<LoadResult, RecvError> {
        self.results.recv()
    }
}

fn run_loader_thread(job_receiver: &Mutex<Receiver<LoadJob>>, result_sender: &Sender<LoadResult>) {
    loop {
        // Holding the lock only while waiting, so the other threads can
        // take jobs while this one decodes. Receiving never panics, so the
        // lock is never poisoned by it.
        let job = job_receiver
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv();
        let Ok(job) = job else {
            // The store was dropped.
            return;
        };
        let finished = match job {
            LoadJob::Texture { texture, path } => LoadResult::Texture {
                texture,
                result: read_and_decode(&path),
            },
            LoadJob::SceneFile { read, path } => LoadResult::SceneFile {
                read,
                result: SceneFileTree::read(&path),
            },
        };
        if result_sender.send(finished).is_err() {
            // The store was dropped while this load ran.
            return;
        }
    }
}

fn read_and_decode(path: &std::path::Path) -> Result<DecodedTexture, TextureLoadError> {
    let bytes = std::fs::read(path).map_err(TextureLoadError::ReadFile)?;
    // The decoder is safe Rust and returns errors for bad files, but a
    // bug in it must fail one load, not stop a loader thread.
    catch_unwind(AssertUnwindSafe(|| decode_png(&bytes)))
        .unwrap_or(Err(TextureLoadError::DecoderPanicked))
}
