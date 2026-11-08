use std::any::TypeId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::bindings::{Binding, InputBindings};
use super::{Action, Axis2d, Button, KeyCode};
use crate::math::Vector2;
use crate::platform::InputEvent;

/// The engine's input: the bindings, which keys are held, and the tick and
/// frame views of the actions (ADR 0008, 0016).
pub(crate) struct InputState {
    bindings: InputBindings,
    keys_held: HashSet<KeyCode>,
    tick_view: InputView,
    frame_view: InputView,
}

impl InputState {
    pub(crate) fn new(bindings: InputBindings) -> InputState {
        let tick_view = InputView::new(&bindings);
        let frame_view = InputView::new(&bindings);
        InputState {
            bindings,
            keys_held: HashSet::new(),
            tick_view,
            frame_view,
        }
    }

    /// Applies one event from the platform right away. Presses and releases
    /// wait in each view until that view is next updated, so neither view
    /// misses them.
    pub(crate) fn apply_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::KeyPressed(key) => self.set_key_held(key, true),
            InputEvent::KeyReleased(key) => self.set_key_held(key, false),
            InputEvent::FocusLost => {
                // The window gets no releases while unfocused, so release
                // everything now rather than leave keys stuck down.
                let held: Vec<KeyCode> = self.keys_held.iter().copied().collect();
                for key in held {
                    self.set_key_held(key, false);
                }
                self.tick_view.pending_focus_lost = true;
                self.frame_view.pending_focus_lost = true;
            }
        }
    }

    /// Moves what happened since the last tick into the tick view. Runs at
    /// the start of each tick.
    pub(crate) fn apply_pending_to_tick_view(&mut self) {
        self.tick_view
            .apply_pending(&self.bindings, &self.keys_held);
    }

    /// Moves what happened since the last frame into the frame view. Runs
    /// before each frame's `frame` callbacks.
    pub(crate) fn apply_pending_to_frame_view(&mut self) {
        self.frame_view
            .apply_pending(&self.bindings, &self.keys_held);
    }

    pub(crate) fn tick_view(&self) -> &InputView {
        &self.tick_view
    }

    pub(crate) fn frame_view(&self) -> &InputView {
        &self.frame_view
    }

    fn set_key_held(&mut self, key: KeyCode, held: bool) {
        // Buttons bound to this key, and whether each was held before.
        let affected: Vec<(usize, bool)> = self
            .bindings
            .actions()
            .iter()
            .enumerate()
            .filter_map(|(index, action)| match &action.binding {
                Binding::Button { keys } if keys.contains(&key) => {
                    Some((index, any_held(keys, &self.keys_held)))
                }
                _ => None,
            })
            .collect();
        if held {
            self.keys_held.insert(key);
        } else {
            self.keys_held.remove(&key);
        }
        for (index, was_held) in affected {
            let Binding::Button { keys } = &self.bindings.actions()[index].binding else {
                unreachable!("only buttons were collected");
            };
            let now_held = any_held(keys, &self.keys_held);
            if now_held != was_held {
                self.tick_view.mark_pending(index, now_held);
                self.frame_view.mark_pending(index, now_held);
            }
        }
    }
}

fn any_held(keys: &[KeyCode], keys_held: &HashSet<KeyCode>) -> bool {
    keys.iter().any(|key| keys_held.contains(key))
}

/// One view of the game's input actions: the tick view in `tick`, or the
/// frame view in `frame` (ADR 0008).
///
/// "Just pressed" and "just released" mean since the last tick in the tick
/// view, and since the last frame in the frame view. Each fires exactly
/// once in each view, even for a tap shorter than a frame, and a press in a
/// frame that runs no ticks shows up on the next tick.
pub struct InputView {
    index_by_type: HashMap<TypeId, usize>,
    /// In the order the bindings list the actions.
    actions: Vec<ActionState>,
    focus_just_lost: bool,
    pending_focus_lost: bool,
    /// Actions asked about that aren't registered, so each is logged once.
    unregistered_reported: RefCell<HashSet<TypeId>>,
}

enum ActionState {
    Button(ButtonState),
    Axis2d(Vector2),
}

#[derive(Default)]
struct ButtonState {
    held: bool,
    just_pressed: bool,
    just_released: bool,
    /// Pressed since this view was last updated.
    pending_press: bool,
    /// Released since this view was last updated.
    pending_release: bool,
}

impl InputView {
    /// A view with no actions, for tests that tick a scene directly.
    #[cfg(test)]
    pub(crate) fn empty() -> InputView {
        InputView::new(&InputBindings::none())
    }

    pub(crate) fn new(bindings: &InputBindings) -> InputView {
        InputView {
            index_by_type: bindings
                .actions()
                .iter()
                .enumerate()
                .map(|(index, action)| (action.type_id, index))
                .collect(),
            actions: bindings
                .actions()
                .iter()
                .map(|action| match action.binding {
                    Binding::Button { .. } => ActionState::Button(ButtonState::default()),
                    Binding::Axis2d { .. } => ActionState::Axis2d(Vector2::ZERO),
                })
                .collect(),
            focus_just_lost: false,
            pending_focus_lost: false,
            unregistered_reported: RefCell::new(HashSet::new()),
        }
    }

    /// Whether any of the button `A`'s keys is held now.
    ///
    /// An action that wasn't registered is never held, and an error is
    /// logged the first time it is asked about.
    pub fn held<A: Action<Kind = Button>>(&self) -> bool {
        self.button::<A>().is_some_and(|button| button.held)
    }

    /// Whether the button `A` went from not held to held since this view
    /// was last updated: since the last tick in the tick view, or the last
    /// frame in the frame view.
    ///
    /// A tap shorter than one update is both just pressed and just released,
    /// and not held.
    pub fn just_pressed<A: Action<Kind = Button>>(&self) -> bool {
        self.button::<A>().is_some_and(|button| button.just_pressed)
    }

    /// Whether the button `A` went from held to not held since this view
    /// was last updated.
    pub fn just_released<A: Action<Kind = Button>>(&self) -> bool {
        self.button::<A>()
            .is_some_and(|button| button.just_released)
    }

    /// The direction of the 2D axis `A`, from −1 to 1 on each axis, with +Y
    /// up. Two directions at once give a diagonal of length 1, unless its
    /// binding sets `normalize = false`.
    ///
    /// An action that wasn't registered is always zero, and an error is
    /// logged the first time it is asked about.
    pub fn axis_2d<A: Action<Kind = Axis2d>>(&self) -> Vector2 {
        match self.state::<A>() {
            Some(ActionState::Axis2d(value)) => *value,
            _ => Vector2::ZERO,
        }
    }

    /// Whether the window lost focus since this view was last updated. When
    /// it does, every held action is released (ADR 0016).
    pub fn focus_just_lost(&self) -> bool {
        self.focus_just_lost
    }

    fn button<A: Action<Kind = Button>>(&self) -> Option<&ButtonState> {
        match self.state::<A>()? {
            ActionState::Button(button) => Some(button),
            ActionState::Axis2d(_) => None,
        }
    }

    fn state<A: Action>(&self) -> Option<&ActionState> {
        let type_id = TypeId::of::<A>();
        let Some(&index) = self.index_by_type.get(&type_id) else {
            if self.unregistered_reported.borrow_mut().insert(type_id) {
                tracing::error!(
                    action = A::NAME,
                    "an action was asked about but isn't registered, so it reads as not pressed; register it with ActionRegistry::register"
                );
            }
            return None;
        };
        Some(&self.actions[index])
    }

    fn mark_pending(&mut self, index: usize, now_held: bool) {
        if let ActionState::Button(button) = &mut self.actions[index] {
            if now_held {
                button.pending_press = true;
            } else {
                button.pending_release = true;
            }
        }
    }

    fn apply_pending(&mut self, bindings: &InputBindings, keys_held: &HashSet<KeyCode>) {
        self.focus_just_lost = std::mem::take(&mut self.pending_focus_lost);
        for (state, action) in self.actions.iter_mut().zip(bindings.actions()) {
            match (state, &action.binding) {
                (ActionState::Button(button), Binding::Button { keys }) => {
                    button.held = any_held(keys, keys_held);
                    button.just_pressed = std::mem::take(&mut button.pending_press);
                    button.just_released = std::mem::take(&mut button.pending_release);
                }
                (
                    ActionState::Axis2d(value),
                    Binding::Axis2d {
                        up,
                        down,
                        left,
                        right,
                        normalize,
                    },
                ) => {
                    let amount = |keys: &[KeyCode]| f32::from(u8::from(any_held(keys, keys_held)));
                    let raw = Vector2::new(amount(right) - amount(left), amount(up) - amount(down));
                    *value = match raw.try_normalize() {
                        Some(unit) if *normalize && raw.length() > 1.0 => unit,
                        _ => raw,
                    };
                }
                _ => unreachable!("views are built from the same bindings"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ActionRegistry;
    use crate::input::test_actions::{Fire, Jump, Move};
    use crate::math::assert_close;

    const BINDINGS: &str = r#"
        [button.jump]
        keys = ["Space", "KeyJ"]

        [axis_2d.move]
        up = ["KeyW"]
        down = ["KeyS"]
        left = ["KeyA"]
        right = ["KeyD"]
    "#;

    fn input_state(bindings: &str) -> InputState {
        let mut actions = ActionRegistry::new();
        actions.register::<Jump>().unwrap();
        actions.register::<Move>().unwrap();
        InputState::new(InputBindings::from_toml(bindings, &actions).unwrap())
    }

    fn press(input: &mut InputState, key: KeyCode) {
        input.apply_event(InputEvent::KeyPressed(key));
    }

    fn release(input: &mut InputState, key: KeyCode) {
        input.apply_event(InputEvent::KeyReleased(key));
    }

    /// (held, just pressed, just released) for jump in the tick view.
    fn tick_jump(input: &InputState) -> (bool, bool, bool) {
        let view = input.tick_view();
        (
            view.held::<Jump>(),
            view.just_pressed::<Jump>(),
            view.just_released::<Jump>(),
        )
    }

    #[test]
    fn nothing_changes_until_the_view_is_updated() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        assert_eq!(tick_jump(&input), (false, false, false));
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (true, true, false));
    }

    #[test]
    fn just_pressed_fires_once_per_view() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        // A frame that runs two ticks: only the first tick sees the press.
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (true, true, false));
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (true, false, false));
        // The frame view sees it once too, on its own schedule.
        input.apply_pending_to_frame_view();
        assert!(input.frame_view().just_pressed::<Jump>());
        input.apply_pending_to_frame_view();
        assert!(!input.frame_view().just_pressed::<Jump>());
        assert!(input.frame_view().held::<Jump>());
    }

    #[test]
    fn a_press_in_a_frame_with_no_ticks_reaches_the_next_tick() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        input.apply_pending_to_frame_view();
        input.apply_pending_to_frame_view();
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (true, true, false));
    }

    #[test]
    fn a_tap_shorter_than_a_tick_is_still_seen() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        release(&mut input, KeyCode::Space);
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (false, true, true));
    }

    #[test]
    fn a_button_is_held_while_any_of_its_keys_is() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        input.apply_pending_to_tick_view();
        // A second key while the first is down isn't a new press.
        press(&mut input, KeyCode::KeyJ);
        release(&mut input, KeyCode::Space);
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (true, false, false));
        release(&mut input, KeyCode::KeyJ);
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (false, false, true));
    }

    #[test]
    fn diagonals_are_normalized_by_default() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::KeyW);
        press(&mut input, KeyCode::KeyD);
        input.apply_pending_to_tick_view();
        let direction = input.tick_view().axis_2d::<Move>();
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        assert_close(direction.x(), diagonal);
        assert_close(direction.y(), diagonal);

        // Opposite directions cancel.
        press(&mut input, KeyCode::KeyS);
        input.apply_pending_to_tick_view();
        assert_eq!(input.tick_view().axis_2d::<Move>(), Vector2::new(1.0, 0.0));
    }

    #[test]
    fn diagonals_can_keep_their_length() {
        let bindings = BINDINGS.replace(
            "right = [\"KeyD\"]",
            "right = [\"KeyD\"]\nnormalize = false",
        );
        let mut input = input_state(&bindings);
        press(&mut input, KeyCode::KeyW);
        press(&mut input, KeyCode::KeyA);
        input.apply_pending_to_tick_view();
        assert_eq!(input.tick_view().axis_2d::<Move>(), Vector2::new(-1.0, 1.0));
    }

    #[test]
    fn losing_focus_releases_everything_and_says_so() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        press(&mut input, KeyCode::KeyW);
        input.apply_pending_to_tick_view();
        input.apply_event(InputEvent::FocusLost);
        input.apply_pending_to_tick_view();
        assert_eq!(tick_jump(&input), (false, false, true));
        assert_eq!(input.tick_view().axis_2d::<Move>(), Vector2::ZERO);
        assert!(input.tick_view().focus_just_lost());
        input.apply_pending_to_tick_view();
        assert!(!input.tick_view().focus_just_lost());
    }

    #[test]
    fn unregistered_actions_read_as_not_pressed() {
        let mut input = input_state(BINDINGS);
        press(&mut input, KeyCode::Space);
        input.apply_pending_to_tick_view();
        assert!(!input.tick_view().held::<Fire>());
        assert!(!input.tick_view().just_pressed::<Fire>());
        // Reported once, not on every ask.
        assert_eq!(input.tick_view().unregistered_reported.borrow().len(), 1);
    }
}
