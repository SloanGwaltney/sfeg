//! SFEG's windowed runner.
//!
//! It owns the window, drives the core once per rendered frame, and hands
//! each frame to `sfeg_wgpu` (ADR 0007). Games start it with [`run`].
//!
//! Key presses and focus changes go to the core as input events
//! (ADR 0016).

mod key_code;

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sfeg_core::platform::InputEvent;
use sfeg_core::{Core, GameState};
use sfeg_wgpu::{Renderer, RendererError};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use crate::key_code::to_sfeg_key_code;

/// How the game's window opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSettings {
    /// The text in the window's title bar.
    pub title: String,
    /// The drawable area's width, in logical pixels. The window is this
    /// size on screen at any display scale, so it has more physical pixels
    /// on a high-DPI display.
    pub logical_width: u32,
    /// The drawable area's height, in logical pixels.
    pub logical_height: u32,
}

/// Opens a window and runs `core` in it until the window is closed
/// (ADR 0006).
///
/// Each rendered frame calls [`Core::advance`] with the real time since the
/// previous frame, then [`Core::draw`], and renders what it drew. Frames are paced by vsync. While the window
/// is hidden, some platforms stop asking for frames, so the game pauses
/// until it is shown again.
///
/// It also calls [`install_default_tracing_subscriber`] (ADR 0009).
///
/// On macOS this must be called on the main thread.
///
/// # Errors
///
/// - [`RunError::EventLoop`] if the platform's event loop couldn't start
///   or failed while running.
/// - [`RunError::CreateWindow`] if the window couldn't open.
/// - [`RunError::Renderer`] if the renderer couldn't start for the window.
pub fn run<G: GameState>(core: Core<G>, window_settings: WindowSettings) -> Result<(), RunError> {
    install_default_tracing_subscriber();

    let event_loop = EventLoop::new().map_err(|source| RunError::EventLoop(Box::new(source)))?;
    let mut runner = Runner {
        core,
        window_settings,
        window: None,
        last_frame_at: None,
        error: None,
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|source| RunError::EventLoop(Box::new(source)))?;
    match runner.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Installs SFEG's default `tracing` subscriber, which logs to stderr
/// (ADR 0009). [`run`] calls it, and headless runs can call it to see the
/// same logs. If a subscriber is already installed, such as one the game
/// chose, that one is kept and this does nothing.
pub fn install_default_tracing_subscriber() {
    // Fails only if a subscriber is already installed.
    let _ = tracing_subscriber::fmt().try_init();
}

struct Runner<G: GameState> {
    core: Core<G>,
    window_settings: WindowSettings,
    window: Option<OpenWindow>,
    /// When the previous frame started. `None` before the first frame.
    last_frame_at: Option<Instant>,
    /// An error from inside the event loop, returned once it exits.
    error: Option<RunError>,
}

struct OpenWindow {
    window: Arc<Window>,
    renderer: Renderer,
}

impl<G: GameState> Runner<G> {
    fn open_window(&self, event_loop: &ActiveEventLoop) -> Result<OpenWindow, RunError> {
        let attributes = Window::default_attributes()
            .with_title(self.window_settings.title.clone())
            .with_inner_size(LogicalSize::new(
                self.window_settings.logical_width,
                self.window_settings.logical_height,
            ));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|source| RunError::CreateWindow(Box::new(source)))?,
        );
        let size = window.inner_size();
        let renderer =
            Renderer::new(window.clone(), size.width, size.height).map_err(RunError::Renderer)?;
        Ok(OpenWindow { window, renderer })
    }

    fn run_frame(&mut self) {
        let Some(open) = &mut self.window else {
            return;
        };
        let now = Instant::now();
        // The first frame has no previous one, so no time has passed.
        let real_dt = self
            .last_frame_at
            .map_or(Duration::ZERO, |last| now.duration_since(last));
        self.last_frame_at = Some(now);

        let summary = self.core.advance(real_dt);
        if summary.ticks_dropped() > 0 {
            tracing::debug!(
                ticks_dropped = summary.ticks_dropped(),
                "frame took longer than the tick cap allows, so the game slowed down"
            );
        }

        let draw_list = self.core.draw();
        open.window.pre_present_notify();
        open.renderer.render_frame(&draw_list);
        // Ask for the next frame right away. Vsync paces them.
        open.window.request_redraw();
    }
}

impl<G: GameState> ApplicationHandler for Runner<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Desktop platforms resume once, at startup.
        if self.window.is_some() {
            return;
        }
        match self.open_window(event_loop) {
            Ok(open) => {
                open.window.request_redraw();
                self.window = Some(open);
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(open) = &mut self.window {
                    open.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.run_frame(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // Synthetic events are winit's guesses about keys held while
                // unfocused. Focus loss already releases everything, and a
                // key held when focus returns counts once it is pressed
                // again.
                if !is_synthetic && let Some(input_event) = to_input_event(&event) {
                    self.core.apply_input_event(input_event);
                }
            }
            WindowEvent::Focused(false) => self.core.apply_input_event(InputEvent::FocusLost),
            _ => {}
        }
    }
}

/// The input event for a key, or `None` for the operating system's key
/// repeat and for keys SFEG doesn't support.
fn to_input_event(event: &KeyEvent) -> Option<InputEvent> {
    if event.repeat {
        return None;
    }
    let PhysicalKey::Code(code) = event.physical_key else {
        return None;
    };
    let key = to_sfeg_key_code(code)?;
    Some(match event.state {
        ElementState::Pressed => InputEvent::KeyPressed(key),
        ElementState::Released => InputEvent::KeyReleased(key),
    })
}

/// Why [`run`] stopped with an error.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    /// The platform's event loop couldn't start or failed while running,
    /// such as on Linux with no display server to connect to.
    EventLoop(Box<dyn Error + Send + Sync>),
    /// The window couldn't open.
    CreateWindow(Box<dyn Error + Send + Sync>),
    /// The renderer couldn't start for the window, such as on a machine
    /// without a working GPU driver.
    Renderer(RendererError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::EventLoop(_) => write!(f, "the window event loop failed"),
            RunError::CreateWindow(_) => write!(f, "couldn't open the window"),
            RunError::Renderer(_) => write!(f, "couldn't start the renderer"),
        }
    }
}

impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RunError::EventLoop(source) | RunError::CreateWindow(source) => Some(source.as_ref()),
            RunError::Renderer(source) => Some(source),
        }
    }
}
