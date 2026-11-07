use std::error::Error;
use std::fmt;

/// How many fixed ticks run per second of game time.
///
/// A tick rate is never zero. Build one with [`TickRate::new`], which
/// returns an error for zero instead of correcting it (ADR 0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickRate {
    hz: u32,
}

impl TickRate {
    /// 60 ticks per second, the default from ADR 0008.
    pub const DEFAULT: TickRate = TickRate { hz: 60 };

    /// Creates a tick rate of `hz` ticks per second.
    ///
    /// # Errors
    ///
    /// Returns [`TickRateError::Zero`] if `hz` is zero.
    pub fn new(hz: u32) -> Result<TickRate, TickRateError> {
        if hz == 0 {
            return Err(TickRateError::Zero);
        }
        Ok(TickRate { hz })
    }

    /// Ticks per second.
    pub fn hz(self) -> u32 {
        self.hz
    }

    /// The fixed length of one tick, in seconds.
    pub fn dt(self) -> f32 {
        1.0 / self.hz as f32
    }
}

/// Why a [`TickRate`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TickRateError {
    /// The tick rate was zero ticks per second.
    Zero,
}

impl fmt::Display for TickRateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TickRateError::Zero => write!(f, "tick rate must be at least 1 tick per second"),
        }
    }
}

impl Error for TickRateError {}

/// The most ticks [`Core::advance`](crate::Core::advance) runs for one
/// frame (ADR 0008).
///
/// When a frame took longer than this many ticks, such as after a stall,
/// the extra time is dropped and the game slows down instead of falling
/// further behind. It is never zero. Build one with
/// [`MaxTicksPerFrame::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxTicksPerFrame {
    ticks: u32,
}

impl MaxTicksPerFrame {
    /// 8 ticks per frame, the same default as Godot's physics steps. At
    /// 60 Hz the game keeps real-time speed down to 7.5 frames per second.
    pub const DEFAULT: MaxTicksPerFrame = MaxTicksPerFrame { ticks: 8 };

    /// Creates a cap of `ticks` ticks per frame.
    ///
    /// # Errors
    ///
    /// Returns [`MaxTicksPerFrameError::Zero`] if `ticks` is zero.
    pub fn new(ticks: u32) -> Result<MaxTicksPerFrame, MaxTicksPerFrameError> {
        if ticks == 0 {
            return Err(MaxTicksPerFrameError::Zero);
        }
        Ok(MaxTicksPerFrame { ticks })
    }

    /// The most ticks one frame runs.
    pub fn ticks(self) -> u32 {
        self.ticks
    }
}

/// Why a [`MaxTicksPerFrame`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaxTicksPerFrameError {
    /// The cap was zero, so no tick could ever run.
    Zero,
}

impl fmt::Display for MaxTicksPerFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaxTicksPerFrameError::Zero => {
                write!(f, "max ticks per frame must be at least 1")
            }
        }
    }
}

impl Error for MaxTicksPerFrameError {}

/// Game time, counted in ticks (ADR 0022).
///
/// This is not real (wall-clock) time. It only moves forward when a tick
/// runs, so a paused or slowed-down game has less game time than real time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameTime {
    tick_count: u64,
    tick_rate: TickRate,
}

impl GameTime {
    pub(crate) fn new(tick_rate: TickRate) -> GameTime {
        GameTime {
            tick_count: 0,
            tick_rate,
        }
    }

    pub(crate) fn advance_one_tick(&mut self) {
        self.tick_count += 1;
    }

    /// The number of ticks that have finished.
    ///
    /// Inside a tick callback this is also the index of the running tick,
    /// so the first tick sees 0.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// The tick rate this game runs at.
    pub fn tick_rate(&self) -> TickRate {
        self.tick_rate
    }

    /// The fixed length of one tick, in seconds.
    pub fn dt(&self) -> f32 {
        self.tick_rate.dt()
    }

    /// Game time in seconds, derived from [`GameTime::tick_count`].
    pub fn elapsed_seconds(&self) -> f64 {
        self.tick_count as f64 / f64::from(self.tick_rate.hz())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_tick_rate_is_an_error() {
        assert_eq!(TickRate::new(0), Err(TickRateError::Zero));
    }

    #[test]
    fn default_tick_rate_is_60_hz() {
        assert_eq!(TickRate::DEFAULT.hz(), 60);
        assert_eq!(TickRate::new(60), Ok(TickRate::DEFAULT));
    }

    #[test]
    fn zero_max_ticks_per_frame_is_an_error() {
        assert_eq!(MaxTicksPerFrame::new(0), Err(MaxTicksPerFrameError::Zero));
        assert_eq!(MaxTicksPerFrame::new(8), Ok(MaxTicksPerFrame::DEFAULT));
    }

    #[test]
    fn elapsed_seconds_is_derived_from_ticks() {
        let mut game_time = GameTime::new(TickRate::DEFAULT);
        for _ in 0..90 {
            game_time.advance_one_tick();
        }
        assert_eq!(game_time.tick_count(), 90);
        assert_eq!(game_time.elapsed_seconds(), 1.5);
    }
}
