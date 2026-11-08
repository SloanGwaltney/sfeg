//! Input actions (ADR 0016).
//!
//! All game input goes through actions. Each action is its own type,
//! declared with `#[derive(Action)]`, and code asks about it by type, such
//! as `ctx.tick_input().just_pressed::<Jump>()`. Using an action as the
//! wrong kind is a compile error. There is no raw input API.
//!
//! Which keys drive each action comes from a TOML bindings file, loaded
//! with [`InputBindings::from_toml`] against an [`ActionRegistry`] of every
//! action the game declares.
//!
//! Following ADR 0008, there are two views of the same input: the tick
//! view, where "just pressed" means since the last tick, and the frame
//! view, where it means since the last frame. Each fires exactly once.

mod bindings;
mod key_code;
mod view;

use std::any::TypeId;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

pub use bindings::{InputBindings, InputBindingsError};
pub use key_code::KeyCode;
pub(crate) use view::InputState;
pub use view::InputView;

/// An input action: something the player wants to do, such as jump or
/// move (ADR 0016).
///
/// Declare one with the derive, which gives it an explicit name for binding
/// files and a kind:
///
/// ```ignore
/// #[derive(Action)]
/// #[action(name = "jump", kind = Button)]
/// struct Jump;
/// ```
///
/// The name is never taken from the type, so renaming the type never
/// breaks saved bindings. Names are lowercase letters, digits, and
/// underscores, starting with a letter.
pub trait Action: 'static {
    /// The action's name in binding files.
    const NAME: &'static str;
    /// What kind of value the action has, such as [`Button`].
    type Kind: ActionKind;
}

/// The kind of value an action has. Only SFEG's kinds implement it.
pub trait ActionKind: sealed::Sealed + 'static {
    /// The kind's table name in binding files, such as `"button"`.
    const TABLE_NAME: &'static str;
}

// Seals `ActionKind` so only SFEG can add kinds. The engine handles each
// kind by hand: bindings only parse the tables for known kinds, and
// `InputView` only has queries for them. A kind from outside SFEG would
// compile and register, but could never be bound or read, so its action
// would silently do nothing. `Sealed` is public inside a private module,
// so other crates can use `ActionKind` in bounds but can't implement it.
// This also lets SFEG add items to `ActionKind` without breaking games.
mod sealed {
    pub trait Sealed {}
}

/// An action that is either held or not, such as jump or fire.
///
/// Bound to keys in a `[button.<name>]` table: `keys = ["Space"]`. It is
/// held while any of its keys is held.
pub enum Button {}

/// An action with a direction, from −1 to 1 on each axis, such as moving.
/// +Y is up.
///
/// Bound in an `[axis_2d.<name>]` table with `up`, `down`, `left`, and
/// `right` key lists. Pressing two directions at once, such as up and
/// right, gives a diagonal of length 1, not 1.41, unless the table sets
/// `normalize = false`.
pub enum Axis2d {}

impl sealed::Sealed for Button {}
impl sealed::Sealed for Axis2d {}

impl ActionKind for Button {
    const TABLE_NAME: &'static str = "button";
}

impl ActionKind for Axis2d {
    const TABLE_NAME: &'static str = "axis_2d";
}

/// Every action the game declares, so its bindings can be checked when they
/// load.
///
/// ```ignore
/// let mut actions = ActionRegistry::new();
/// actions.register::<Jump>()?;
/// actions.register::<Move>()?;
/// let bindings = InputBindings::from_toml(include_str!("bindings.toml"), &actions)?;
/// ```
#[derive(Debug, Default)]
pub struct ActionRegistry {
    /// In the order they were registered.
    actions: Vec<RegisteredAction>,
}

#[derive(Debug, Clone, Copy)]
struct RegisteredAction {
    type_id: TypeId,
    name: &'static str,
    kind_table_name: &'static str,
}

impl ActionRegistry {
    /// A registry with no actions.
    pub fn new() -> ActionRegistry {
        ActionRegistry::default()
    }

    /// Adds the action `A`.
    ///
    /// # Errors
    ///
    /// - [`ActionRegistryError::AlreadyRegistered`] if `A` was registered
    ///   before.
    /// - [`ActionRegistryError::DuplicateName`] if another action has the
    ///   same name.
    /// - [`ActionRegistryError::InvalidName`] if the name isn't lowercase
    ///   letters, digits, and underscores, starting with a letter. The
    ///   derive already checks this at compile time.
    pub fn register<A: Action>(&mut self) -> Result<(), ActionRegistryError> {
        let type_id = TypeId::of::<A>();
        if self.actions.iter().any(|action| action.type_id == type_id) {
            return Err(ActionRegistryError::AlreadyRegistered { name: A::NAME });
        }
        if self.actions.iter().any(|action| action.name == A::NAME) {
            return Err(ActionRegistryError::DuplicateName { name: A::NAME });
        }
        if !is_valid_action_name(A::NAME) {
            return Err(ActionRegistryError::InvalidName { name: A::NAME });
        }
        self.actions.push(RegisteredAction {
            type_id,
            name: A::NAME,
            kind_table_name: <A::Kind as ActionKind>::TABLE_NAME,
        });
        Ok(())
    }

    fn by_name(&self) -> HashMap<&'static str, RegisteredAction> {
        self.actions
            .iter()
            .map(|action| (action.name, *action))
            .collect()
    }
}

/// Whether `name` is lowercase letters, digits, and underscores, starting
/// with a letter.
fn is_valid_action_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Why an action could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionRegistryError {
    /// The same action type was registered twice.
    AlreadyRegistered {
        /// The action's name.
        name: &'static str,
    },
    /// Two different action types declare the same name.
    DuplicateName {
        /// The name both declare.
        name: &'static str,
    },
    /// The name has characters other than lowercase letters, digits, and
    /// underscores, or doesn't start with a letter.
    InvalidName {
        /// The name.
        name: &'static str,
    },
}

impl fmt::Display for ActionRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionRegistryError::AlreadyRegistered { name } => {
                write!(f, "the action `{name}` is already registered")
            }
            ActionRegistryError::DuplicateName { name } => {
                write!(f, "two different actions are named `{name}`")
            }
            ActionRegistryError::InvalidName { name } => write!(
                f,
                "the action name `{name}` must be lowercase letters, digits, and underscores, starting with a letter"
            ),
        }
    }
}

impl Error for ActionRegistryError {}

#[cfg(test)]
pub(crate) mod test_actions {
    use super::{Action, Axis2d, Button};

    pub(crate) struct Jump;

    impl Action for Jump {
        const NAME: &'static str = "jump";
        type Kind = Button;
    }

    pub(crate) struct Move;

    impl Action for Move {
        const NAME: &'static str = "move";
        type Kind = Axis2d;
    }

    /// Never registered.
    pub(crate) struct Fire;

    impl Action for Fire {
        const NAME: &'static str = "fire";
        type Kind = Button;
    }
}

#[cfg(test)]
mod tests {
    use super::test_actions::{Jump, Move};
    use super::*;

    struct AlsoJump;

    impl Action for AlsoJump {
        const NAME: &'static str = "jump";
        type Kind = Button;
    }

    struct BadName;

    impl Action for BadName {
        const NAME: &'static str = "Jump!";
        type Kind = Button;
    }

    #[test]
    fn registering_checks_types_and_names() {
        let mut actions = ActionRegistry::new();
        actions.register::<Jump>().unwrap();
        actions.register::<Move>().unwrap();
        assert_eq!(
            actions.register::<Jump>(),
            Err(ActionRegistryError::AlreadyRegistered { name: "jump" })
        );
        assert_eq!(
            actions.register::<AlsoJump>(),
            Err(ActionRegistryError::DuplicateName { name: "jump" })
        );
        assert_eq!(
            actions.register::<BadName>(),
            Err(ActionRegistryError::InvalidName { name: "Jump!" })
        );
    }

    #[test]
    fn action_names_are_lowercase_snake_case() {
        for good in ["jump", "move_2", "a"] {
            assert!(is_valid_action_name(good), "{good}");
        }
        for bad in ["", "Jump", "2jump", "_jump", "jump-now", "jump now"] {
            assert!(!is_valid_action_name(bad), "{bad}");
        }
    }
}
