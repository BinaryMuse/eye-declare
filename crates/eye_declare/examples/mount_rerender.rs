//! Demonstrates that use_mount state changes should trigger a re-render.
//!
//! A component uses use_mount to set a label in state. The label should
//! be visible immediately without needing any external event to trigger
//! a re-render.
//!
//! BUG: The label shows "[mount has not fired yet]" until an external
//! event causes a re-render, even though mount fires and sets state.
//!
//! Run with: cargo run --example mount_rerender

use std::io;
use std::time::Duration;

use eye_declare::{Application, Elements, Hooks, Span, Text, component, element, props};
use ratatui_core::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Component that sets its label via use_mount
// ---------------------------------------------------------------------------

#[props]
struct MountLabel {
    value: String,
}

#[derive(Default)]
struct MountLabelState {
    label: Option<String>,
}

#[component(props = MountLabel, state = MountLabelState)]
fn mount_label(
    _props: &MountLabel,
    state: &MountLabelState,
    hooks: &mut Hooks<MountLabel, MountLabelState>,
) -> Elements {
    hooks.use_mount(|props, state| {
        state.label = Some(format!("Mounted with: {}", props.value));
    });

    let (text, style) = match &state.label {
        Some(label) => (
            label.clone(),
            Style::default().fg(Color::Green),
        ),
        None => (
            "[mount has not fired yet]".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    };

    element! {
        Text {
            Span(text: text, style: style)
        }
    }
}

// ---------------------------------------------------------------------------
// App state + view
// ---------------------------------------------------------------------------

struct AppState {
    show_component: bool,
}

fn app_view(state: &AppState) -> Elements {
    element! {
        Text {
            Span(
                text: "Mount re-render test",
                style: Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )
        }

        #(if state.show_component {
            MountLabel(key: "mount-label", value: "hello from mount")
        })

        Text {
            Span(text: "---", style: Style::default().fg(Color::DarkGray))
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState {
            show_component: false,
        })
        .view(app_view)
        .build()?;

    tokio::spawn(async move {
        // Step 1: Show the component — mount should fire and set the label
        handle.update(|s| {
            s.show_component = true;
        });

        // Wait to observe — if the label says "[mount has not fired yet]",
        // the bug is present. It should say "Mounted with: hello from mount".
        tokio::time::sleep(Duration::from_secs(3)).await;

        // handle dropped → app exits
    });

    app.run().await?;

    println!();
    Ok(())
}
