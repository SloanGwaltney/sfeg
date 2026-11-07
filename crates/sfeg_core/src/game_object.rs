use std::any::Any;

use crate::scene::TickContext;

/// A thing in a scene, such as a player, an enemy, or a door (ADR 0004).
///
/// Each object has its own state as fields and its own behavior as
/// callbacks. Its transform is stored by the engine, not in the struct, and
/// is reached through the context, such as [`TickContext::local`]
/// (ADR 0013).
///
/// Objects refer to each other by [`Handle`](crate::Handle), never by
/// reference.
pub trait GameObject: Any {
    /// Runs once per fixed tick and changes the game (ADR 0008).
    ///
    /// Objects tick in a fixed order, but the order isn't otherwise
    /// guaranteed. During its own tick an object is taken out of the scene,
    /// so looking itself up by handle finds nothing; it already has
    /// `&mut self`. Its transform stays in the scene and is reachable
    /// through the context.
    fn tick(&mut self, ctx: &mut TickContext) {
        let _ = ctx;
    }
}
