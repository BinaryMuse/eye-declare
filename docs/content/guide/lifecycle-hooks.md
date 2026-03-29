---
title: Lifecycle Hooks
description: Intervals, mount/unmount effects, and the lifecycle system
---

# Lifecycle Hooks

Components declare side effects through the `lifecycle()` method. The framework manages registration, execution, and cleanup automatically.

## How lifecycle works

The framework calls `lifecycle()` after every build and update. Each call collects a fresh set of effects — the framework clears old effects and installs the new ones. This means effects are always consistent with current props and state.

```rust
impl Component for Timer {
    type State = TimerState;

    fn lifecycle(&self, hooks: &mut Hooks<TimerState>, _state: &TimerState) {
        if self.running {
            hooks.use_interval(Duration::from_secs(1), |s| s.elapsed += 1);
        }
        hooks.use_mount(|s| s.started_at = Instant::now());
        hooks.use_unmount(|s| println!("Timer ran for {:?}", s.started_at.elapsed()));
    }
}
```

Notice that the interval is conditional — when `self.running` changes to `false`, the next `lifecycle()` call simply doesn't register the interval, and the framework stops it.

## Available hooks

### use_interval

Fires periodically at the given duration during the framework's tick cycle:

```rust
hooks.use_interval(Duration::from_millis(80), |state| {
    state.frame = state.frame.wrapping_add(1);
});
```

The handler receives `&mut State` — any mutation automatically marks the component dirty for re-rendering. This is how the built-in `Spinner` animates: it registers an 80ms interval that cycles through Braille frames.

### use_mount

Fires once after the component is first built:

```rust
hooks.use_mount(|state| {
    state.created_at = Instant::now();
    state.entries.push("Component mounted".into());
});
```

Use this for one-time initialization that depends on state being available.

### use_unmount

Fires once when the component is removed from the tree:

```rust
hooks.use_unmount(|state| {
    println!("Component lived for {:?}", state.created_at.elapsed());
});
```

Use this for cleanup: logging, recording metrics, etc. Note that the handler receives `&mut State` — you can still read state during unmount.

### use_autofocus

Requests focus when the component mounts:

```rust
hooks.use_autofocus();
```

If multiple components mount with autofocus in the same rebuild, the last one wins. This is typically used for input fields that should be focused on creation.

### use_focus_scope

Marks this component as a focus scope boundary. Tab/Shift-Tab cycling is confined to focusable descendants within the scope:

```rust
hooks.use_focus_scope();
```

Scopes nest — an inner scope takes precedence over an outer one. When the scope is removed, focus returns to wherever it was before the scope captured it. See [Events and Focus](events-and-focus.md#focus-scopes) for full details.

### provide_context

Makes a typed value available to all descendant components. See [Context](context.md) for details:

```rust
hooks.provide_context(self.theme.clone());
```

### use_context

Reads a value provided by an ancestor. See [Context](context.md) for details:

```rust
hooks.use_context::<Theme>(|theme, state| {
    state.current_theme = theme.cloned();
});
```

## Effect lifecycle

Here's the full sequence when a component is built or updated:

1. `lifecycle()` is called with a fresh `Hooks` instance
2. The component registers its effects via the hooks API
3. Old effects are cleared
4. New effects are installed
5. `use_mount` fires (only on first build)
6. `use_context` handlers fire (after `lifecycle()` returns)

When a component is removed:

1. `use_unmount` fires
2. All effects (including intervals) are cleaned up
3. The node is tombstoned and its slot freed for reuse

## Conditional effects

Because `lifecycle()` runs on every rebuild, you can conditionally register effects:

```rust
fn lifecycle(&self, hooks: &mut Hooks<MyState>, state: &MyState) {
    // Only animate when visible
    if self.visible {
        hooks.use_interval(Duration::from_millis(100), |s| {
            s.animation_frame += 1;
        });
    }

    // Always track mount/unmount
    hooks.use_mount(|s| s.log("mounted"));
    hooks.use_unmount(|s| s.log("unmounted"));
}
```

When `self.visible` changes from `true` to `false`, the interval stops on the next rebuild. When it changes back, a new interval starts.

## Example: StatusLog component

This example from the `lifecycle` example shows a component that records its own lifecycle events:

```rust
struct StatusLog {
    name: String,
}

#[derive(Default)]
struct StatusLogState {
    entries: Vec<(String, Style)>,
}

impl Component for StatusLog {
    type State = StatusLogState;

    fn initial_state(&self) -> Option<StatusLogState> {
        let mut state = StatusLogState::default();
        state.entries.push((
            format!("  {} created", self.name),
            Style::default().fg(Color::DarkGray),
        ));
        Some(state)
    }

    fn lifecycle(&self, hooks: &mut Hooks<StatusLogState>, _state: &StatusLogState) {
        let mount_name = self.name.clone();
        hooks.use_mount(move |state| {
            state.entries.push((
                format!("  {} mounted", mount_name),
                Style::default().fg(Color::Green),
            ));
        });

        let unmount_name = self.name.clone();
        hooks.use_unmount(move |state| {
            state.entries.push((
                format!("  {} unmounted", unmount_name),
                Style::default().fg(Color::Red),
            ));
        });
    }

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let lines: Vec<Line> = state.entries.iter()
            .map(|(text, style)| Line::from(Span::styled(text.as_str(), *style)))
            .collect();
        Paragraph::new(lines).render(area, buf);
    }
}
```

## Behavioral hooks

In addition to lifecycle effects, hooks can declare behavioral capabilities
for the component. These override the equivalent Component trait methods:

### use_focusable

Declares whether this component participates in Tab cycling:

```rust
hooks.use_focusable(true);
```

### use_cursor

Declares where to position the terminal cursor when this component has focus:

```rust
hooks.use_cursor(|area: Rect, state: &MyState| {
    Some((area.x + state.cursor_col, area.y))
});
```

Return `None` to hide the cursor.

### use_event

Declares a bubble-phase event handler (focused → root):

```rust
hooks.use_event(|event: &crossterm::event::Event, state: &mut MyState| {
    if let Event::Key(key) = event {
        state.handle_key(key);
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
});
```

### use_event_capture

Declares a capture-phase event handler (root → focused). Fires before
the bubble phase — return `Consumed` to prevent the event from reaching
the focused component:

```rust
hooks.use_event_capture(|event, state: &mut MyState| {
    // Global shortcut handling
    EventResult::Ignored
});
```

## Hook reference

| Hook | Purpose | Receives |
|------|---------|----------|
| `use_interval(duration, handler)` | Periodic callback | `&mut State` |
| `use_mount(handler)` | Fire on first build | `&mut State` |
| `use_unmount(handler)` | Fire on removal | `&mut State` |
| `use_autofocus()` | Request focus on mount | — |
| `use_focus_scope()` | Create focus scope boundary | — |
| `use_focusable(bool)` | Participate in Tab cycling | — |
| `use_cursor(handler)` | Position cursor when focused | `Rect, &State` → `Option<(u16, u16)>` |
| `use_event(handler)` | Handle events (bubble phase) | `&Event, &mut State` → `EventResult` |
| `use_event_capture(handler)` | Handle events (capture phase) | `&Event, &mut State` → `EventResult` |
| `provide_context(value)` | Make value available to descendants | — |
| `use_context::<T>(handler)` | Read ancestor context | `Option<&T>, &mut Tracked<S>` |
