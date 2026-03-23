use crate::components::spinner::{Spinner, SpinnerState};
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`Spinner`] component.
///
/// Spinners automatically animate via the component's `lifecycle()`
/// method — no manual tick registration needed. The Spinner component
/// declares its own interval effect when not done.
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
        }
        // Effects managed by Spinner::lifecycle() — no imperative registration
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
        // Effects re-registered by Spinner::lifecycle() after update.
    }
}
