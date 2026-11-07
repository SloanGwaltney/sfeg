use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;

use serde::Deserialize;

use super::{ActionKind, ActionRegistry, Axis2d, Button, KeyCode};

/// Which keys drive each action, loaded from a TOML bindings file
/// (ADR 0016).
///
/// Each action has a table under its kind, named by the action's declared
/// name:
///
/// ```toml
/// [button.jump]
/// keys = ["Space"]
///
/// [axis_2d.move]
/// up = ["KeyW", "ArrowUp"]
/// down = ["KeyS", "ArrowDown"]
/// left = ["KeyA", "ArrowLeft"]
/// right = ["KeyD", "ArrowRight"]
/// # normalize = false  # to keep diagonals at length 1.41
/// ```
///
/// Keys are physical positions, named as in [`KeyCode`]. An empty key list
/// leaves the action unbound on purpose.
#[derive(Debug, Clone, PartialEq)]
pub struct InputBindings {
    /// In the order the actions were registered.
    actions: Vec<BoundAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundAction {
    pub(crate) type_id: TypeId,
    pub(crate) name: &'static str,
    pub(crate) binding: Binding,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Binding {
    Button {
        keys: Vec<KeyCode>,
    },
    Axis2d {
        up: Vec<KeyCode>,
        down: Vec<KeyCode>,
        left: Vec<KeyCode>,
        right: Vec<KeyCode>,
        normalize: bool,
    },
}

impl InputBindings {
    /// No actions and no bindings, for a game or test that takes no input.
    pub fn none() -> InputBindings {
        InputBindings {
            actions: Vec::new(),
        }
    }

    /// Loads the bindings in `toml_text` for the actions in `actions`.
    ///
    /// Until resource loading exists, games pass the file's text, such as
    /// with `include_str!("bindings.toml")`.
    ///
    /// # Errors
    ///
    /// - [`InputBindingsError::Syntax`] if the text isn't valid TOML or
    ///   doesn't follow the bindings format.
    /// - [`InputBindingsError::UnknownAction`] if the file binds an action
    ///   that isn't registered.
    /// - [`InputBindingsError::WrongKind`] if an action's table is under a
    ///   different kind than the action declares.
    /// - [`InputBindingsError::UnknownKey`] if a key name isn't a
    ///   [`KeyCode`].
    /// - [`InputBindingsError::Unbound`] if a registered action has no
    ///   table in the file.
    pub fn from_toml(
        toml_text: &str,
        actions: &ActionRegistry,
    ) -> Result<InputBindings, InputBindingsError> {
        let file: BindingsFile =
            toml::from_str(toml_text).map_err(|error| InputBindingsError::Syntax {
                message: error.to_string(),
            })?;
        let registered = actions.by_name();
        let mut bindings: HashMap<&'static str, Binding> = HashMap::new();

        let mut bind = |name: &str,
                        file_kind: &'static str,
                        to_binding: &dyn Fn() -> Result<Binding, InputBindingsError>|
         -> Result<(), InputBindingsError> {
            let action = registered
                .get(name)
                .ok_or_else(|| InputBindingsError::UnknownAction {
                    action: name.to_string(),
                })?;
            if action.kind_table_name != file_kind {
                return Err(InputBindingsError::WrongKind {
                    action: action.name,
                    declared_kind: action.kind_table_name,
                    file_kind,
                });
            }
            bindings.insert(action.name, to_binding()?);
            Ok(())
        };

        for (name, table) in &file.button {
            bind(name, Button::TABLE_NAME, &|| {
                Ok(Binding::Button {
                    keys: parse_keys(name, &table.keys)?,
                })
            })?;
        }
        for (name, table) in &file.axis_2d {
            bind(name, Axis2d::TABLE_NAME, &|| {
                Ok(Binding::Axis2d {
                    up: parse_keys(name, &table.up)?,
                    down: parse_keys(name, &table.down)?,
                    left: parse_keys(name, &table.left)?,
                    right: parse_keys(name, &table.right)?,
                    normalize: table.normalize,
                })
            })?;
        }

        let actions = actions
            .actions
            .iter()
            .map(|action| {
                let binding = bindings
                    .remove(action.name)
                    .ok_or(InputBindingsError::Unbound {
                        action: action.name,
                    })?;
                Ok(BoundAction {
                    type_id: action.type_id,
                    name: action.name,
                    binding,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(InputBindings { actions })
    }

    pub(crate) fn actions(&self) -> &[BoundAction] {
        &self.actions
    }
}

fn parse_keys(action: &str, names: &[String]) -> Result<Vec<KeyCode>, InputBindingsError> {
    names
        .iter()
        .map(|name| {
            KeyCode::from_name(name).ok_or_else(|| InputBindingsError::UnknownKey {
                action: action.to_string(),
                key: name.clone(),
            })
        })
        .collect()
}

/// The file's layout. Sorted maps, so errors come out in the same order
/// every time.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingsFile {
    #[serde(default)]
    button: BTreeMap<String, ButtonTable>,
    #[serde(default)]
    axis_2d: BTreeMap<String, Axis2dTable>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonTable {
    keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Axis2dTable {
    up: Vec<String>,
    down: Vec<String>,
    left: Vec<String>,
    right: Vec<String>,
    #[serde(default = "normalize_by_default")]
    normalize: bool,
}

fn normalize_by_default() -> bool {
    true
}

/// Why a bindings file could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputBindingsError {
    /// The text isn't valid TOML, or has a table or field the bindings
    /// format doesn't have.
    Syntax {
        /// What the TOML parser reported, with the line.
        message: String,
    },
    /// The file binds an action the game didn't register.
    UnknownAction {
        /// The name in the file.
        action: String,
    },
    /// The file binds an action under a different kind than it declares,
    /// such as a button's keys for a 2D axis.
    WrongKind {
        /// The action's name.
        action: &'static str,
        /// The kind the action declares, such as `"axis_2d"`.
        declared_kind: &'static str,
        /// The kind the file binds it as, such as `"button"`.
        file_kind: &'static str,
    },
    /// A key name isn't one SFEG knows.
    UnknownKey {
        /// The action the key is bound to.
        action: String,
        /// The name in the file.
        key: String,
    },
    /// A registered action has no table in the file.
    Unbound {
        /// The action's name.
        action: &'static str,
    },
}

impl fmt::Display for InputBindingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputBindingsError::Syntax { message } => {
                write!(f, "the bindings file is malformed: {message}")
            }
            InputBindingsError::UnknownAction { action } => {
                write!(
                    f,
                    "the bindings file binds `{action}`, which isn't a registered action"
                )
            }
            InputBindingsError::WrongKind {
                action,
                declared_kind,
                file_kind,
            } => write!(
                f,
                "the bindings file binds `{action}` as `{file_kind}`, but it is declared as `{declared_kind}`"
            ),
            InputBindingsError::UnknownKey { action, key } => {
                write!(f, "the bindings for `{action}` name an unknown key `{key}`")
            }
            InputBindingsError::Unbound { action } => {
                write!(f, "the bindings file has no binding for `{action}`")
            }
        }
    }
}

impl Error for InputBindingsError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::test_actions::{Jump, Move};

    const BINDINGS: &str = r#"
        [button.jump]
        keys = ["Space", "KeyJ"]

        [axis_2d.move]
        up = ["KeyW"]
        down = ["KeyS"]
        left = ["KeyA"]
        right = ["KeyD"]
    "#;

    fn registry() -> ActionRegistry {
        let mut actions = ActionRegistry::new();
        actions.register::<Jump>().unwrap();
        actions.register::<Move>().unwrap();
        actions
    }

    #[test]
    fn loads_every_registered_action_in_order() {
        let bindings = InputBindings::from_toml(BINDINGS, &registry()).unwrap();
        let actions = bindings.actions();
        assert_eq!(actions[0].name, "jump");
        assert_eq!(
            actions[0].binding,
            Binding::Button {
                keys: vec![KeyCode::Space, KeyCode::KeyJ]
            }
        );
        assert_eq!(actions[1].name, "move");
        assert!(matches!(
            actions[1].binding,
            Binding::Axis2d {
                normalize: true,
                ..
            }
        ));
    }

    #[test]
    fn normalize_can_be_turned_off() {
        let text = BINDINGS.replace(
            "right = [\"KeyD\"]",
            "right = [\"KeyD\"]\nnormalize = false",
        );
        let bindings = InputBindings::from_toml(&text, &registry()).unwrap();
        assert!(matches!(
            bindings.actions()[1].binding,
            Binding::Axis2d {
                normalize: false,
                ..
            }
        ));
    }

    #[test]
    fn a_binding_of_the_wrong_kind_is_an_error() {
        let text = r#"
            [button.jump]
            keys = ["Space"]

            [button.move]
            keys = ["KeyW"]
        "#;
        assert_eq!(
            InputBindings::from_toml(text, &registry()),
            Err(InputBindingsError::WrongKind {
                action: "move",
                declared_kind: "axis_2d",
                file_kind: "button",
            })
        );
    }

    #[test]
    fn unknown_actions_and_keys_are_errors() {
        let text = format!("{BINDINGS}\n[button.jupm]\nkeys = [\"Space\"]");
        assert_eq!(
            InputBindings::from_toml(&text, &registry()),
            Err(InputBindingsError::UnknownAction {
                action: "jupm".to_string()
            })
        );
        let text = BINDINGS.replace("KeyJ", "J");
        assert_eq!(
            InputBindings::from_toml(&text, &registry()),
            Err(InputBindingsError::UnknownKey {
                action: "jump".to_string(),
                key: "J".to_string()
            })
        );
    }

    #[test]
    fn every_registered_action_must_be_bound() {
        let text = "[button.jump]\nkeys = []";
        assert_eq!(
            InputBindings::from_toml(text, &registry()),
            Err(InputBindingsError::Unbound { action: "move" })
        );
    }

    #[test]
    fn malformed_files_are_syntax_errors() {
        for text in [
            "[button.jump",
            "[button.jump]\nkey = [\"Space\"]",
            "[buttons.jump]\nkeys = []",
            "[axis_2d.move]\nup = [\"KeyW\"]",
        ] {
            let result = InputBindings::from_toml(text, &registry());
            assert!(
                matches!(result, Err(InputBindingsError::Syntax { .. })),
                "{text}: {result:?}"
            );
        }
    }
}
