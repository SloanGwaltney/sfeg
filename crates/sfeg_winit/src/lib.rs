//! SFEG's windowed runner.
//!
//! It will own the window, turn winit events into input data, drive the
//! core, and hand draw commands to `sfeg_wgpu` (ADR 0007). No window is
//! opened yet.
