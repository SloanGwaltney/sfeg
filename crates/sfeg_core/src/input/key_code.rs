/// Declares [`KeyCode`] with each key's name in binding files.
macro_rules! key_codes {
    ($($(#[$doc:meta])* $key:ident),* $(,)?) => {
        /// A physical key position on the keyboard, named for the key in
        /// that place on a US QWERTY layout (ADR 0016).
        ///
        /// On a French AZERTY keyboard, [`KeyCode::KeyW`] is the key labeled
        /// Z, so default controls land in the same place on every layout.
        /// Binding files use the same names, such as `"KeyW"` and
        /// `"ArrowUp"`, which match the web's `KeyboardEvent.code`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum KeyCode {
            $($(#[$doc])* $key,)*
        }

        impl KeyCode {
            /// The key's name in binding files, such as `"KeyW"`.
            pub fn name(self) -> &'static str {
                match self {
                    $(KeyCode::$key => stringify!($key),)*
                }
            }

            /// The key a binding file's name refers to, or `None` if no key
            /// has that name. Names are case-sensitive.
            pub fn from_name(name: &str) -> Option<KeyCode> {
                match name {
                    $(stringify!($key) => Some(KeyCode::$key),)*
                    _ => None,
                }
            }
        }
    };
}

key_codes! {
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Space, Enter, Escape, Tab, Backspace,
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for key in [
            KeyCode::KeyW,
            KeyCode::Digit0,
            KeyCode::ArrowUp,
            KeyCode::F12,
        ] {
            assert_eq!(KeyCode::from_name(key.name()), Some(key));
        }
        assert_eq!(KeyCode::KeyW.name(), "KeyW");
    }

    #[test]
    fn unknown_and_miscased_names_are_none() {
        assert_eq!(KeyCode::from_name("keyw"), None);
        assert_eq!(KeyCode::from_name("W"), None);
    }
}
