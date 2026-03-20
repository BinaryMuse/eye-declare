use std::time::Duration;

use crate::components::spinner::{Spinner, SpinnerState};
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`Spinner`] component.
///
/// Spinners automatically animate when built — no manual ticking
/// needed. The framework calls the registered tick handler at 80ms
/// intervals. When the spinner is done, the tick is unregistered.
///
/// ```ignore
/// els.add(SpinnerEl::new("Thinking..."));
/// els.add(SpinnerEl::new("Done").done("Completed!"));
/// ```
pub struct SpinnerEl {
    label: String,
    done: bool,
    done_label: Option<String>,
}

impl SpinnerEl {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            done: false,
            done_label: None,
        }
    }

    /// Mark the spinner as already done, with a completion label.
    pub fn done(mut self, label: impl Into<String>) -> Self {
        self.done = true;
        self.done_label = Some(label.into());
        self
    }
}

impl Element for SpinnerEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, Spinner);
        let state = renderer.state_mut::<Spinner>(id);
        **state = SpinnerState::new(self.label);
        if self.done {
            state.complete(self.done_label);
        } else {
            renderer.register_tick::<Spinner>(id, Duration::from_millis(80), |state| {
                state.tick();
            });
        }
        id
    }

    fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
        let state = renderer.state_mut::<Spinner>(node_id);
        // Props (parent-controlled): update on every rebuild
        state.label = self.label;
        state.done = self.done;
        state.done_label = self.done_label;
        // Local state (component-internal): frame, spinner_style,
        // label_style, done_style are intentionally NOT reset here.

        // Manage tick registration based on done state
        if self.done {
            renderer.unregister_tick(node_id);
        } else {
            renderer.register_tick::<Spinner>(node_id, Duration::from_millis(80), |state| {
                state.tick();
            });
        }
    }
}
