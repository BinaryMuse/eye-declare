use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::component::VStack;
use crate::element::Elements;
use crate::inline::InlineRenderer;
use crate::node::NodeId;

/// Controls whether the application event loop continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    /// Continue running the event loop.
    Continue,
    /// Exit the event loop.
    Exit,
}

/// A handle for sending state updates to a running [`Application`]
/// from other threads or async tasks.
///
/// `Handle` is `Send + Clone`. Clone it for each thread or task
/// that needs to send updates. Updates are applied on the next
/// frame before rebuild.
///
/// ```ignore
/// let h = handle.clone();
/// tokio::spawn(async move {
///     h.update(|state| state.response.push_str(&token));
/// });
/// ```
pub struct Handle<S: Send + 'static> {
    tx: mpsc::Sender<Box<dyn FnOnce(&mut S) + Send>>,
    exit: Arc<AtomicBool>,
}

impl<S: Send + 'static> Handle<S> {
    /// Queue a state mutation. Applied on the next frame.
    pub fn update(&self, f: impl FnOnce(&mut S) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }

    /// Signal the application to exit its event loop.
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Release);
    }
}

impl<S: Send + 'static> Clone for Handle<S> {
    fn clone(&self) -> Self {
        Handle {
            tx: self.tx.clone(),
            exit: self.exit.clone(),
        }
    }
}

/// Builder for configuring an [`Application`].
pub struct ApplicationBuilder<S: Send + 'static> {
    state: Option<S>,
    view_fn: Option<Box<dyn Fn(&S) -> Elements>>,
    width: Option<u16>,
}

impl<S: Send + 'static> ApplicationBuilder<S> {
    /// Set the initial application state.
    pub fn state(mut self, state: S) -> Self {
        self.state = Some(state);
        self
    }

    /// Set the view function that produces elements from state.
    pub fn view(mut self, f: impl Fn(&S) -> Elements + 'static) -> Self {
        self.view_fn = Some(Box::new(f));
        self
    }

    /// Override the terminal width. Defaults to the current terminal
    /// width if not specified.
    pub fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// Build the Application and its Handle.
    ///
    /// Queries terminal size if width was not specified, which may
    /// fail if no terminal is attached.
    pub fn build(self) -> io::Result<(Application<S>, Handle<S>)> {
        let state = self.state.expect("Application requires .state()");
        let view_fn = self.view_fn.expect("Application requires .view()");
        let width = match self.width {
            Some(w) => w,
            None => crossterm::terminal::size()?.0,
        };

        let (tx, rx) = mpsc::channel();
        let exit = Arc::new(AtomicBool::new(false));
        let handle = Handle {
            tx,
            exit: exit.clone(),
        };

        let mut inline = InlineRenderer::new(width);
        let container = inline.push(VStack);

        let app = Application {
            state,
            view_fn,
            inline,
            container,
            dirty: true,
            rx,
            exit,
        };

        Ok((app, handle))
    }
}

/// An application that owns state, a view function, and a renderer.
///
/// Provides two usage modes:
///
/// **Framework-owned loop** via [`run()`](Application::run):
/// ```ignore
/// let (mut app, handle) = Application::builder()
///     .state(MyState::new())
///     .view(my_view)
///     .build()?;
///
/// app.run(|event, state| {
///     ControlFlow::Continue
/// })?;
/// ```
///
/// **Step API** for custom loops:
/// ```ignore
/// app.update(|s| s.count += 1);
/// app.tick();
/// app.flush(&mut stdout)?;
/// ```
pub struct Application<S: Send + 'static> {
    state: S,
    view_fn: Box<dyn Fn(&S) -> Elements>,
    inline: InlineRenderer,
    container: NodeId,
    dirty: bool,
    rx: mpsc::Receiver<Box<dyn FnOnce(&mut S) + Send>>,
    exit: Arc<AtomicBool>,
}

impl<S: Send + 'static> Application<S> {
    /// Create a new [`ApplicationBuilder`].
    pub fn builder() -> ApplicationBuilder<S> {
        ApplicationBuilder {
            state: None,
            view_fn: None,
            width: None,
        }
    }

    /// Run the interactive event loop.
    ///
    /// Enables terminal raw mode, polls events, ticks effects,
    /// and renders. The handler receives terminal events and
    /// mutable state; return [`ControlFlow::Exit`] to stop.
    /// Ctrl+C always exits.
    ///
    /// Handle updates from other threads are drained each frame.
    pub fn run(
        &mut self,
        mut handler: impl FnMut(&Event, &mut S) -> ControlFlow,
    ) -> io::Result<()> {
        let mut stdout = io::stdout();

        // Initial build + render before entering raw mode
        self.rebuild();
        self.flush_to(&mut stdout)?;

        crossterm::terminal::enable_raw_mode()?;
        let result = self.event_loop(&mut handler, &mut stdout);
        crossterm::terminal::disable_raw_mode()?;

        // Show cursor and newline for clean terminal state
        stdout.write_all(b"\x1b[?25h")?;
        writeln!(stdout)?;

        result
    }

    /// Run the tick/render loop until no effects are active.
    ///
    /// Useful for non-interactive animations (e.g., spinners that
    /// complete on their own). Does not poll terminal events or
    /// enable raw mode.
    pub fn run_while_active(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.flush(writer)?;
        while self.has_active() {
            std::thread::sleep(Duration::from_millis(16));
            self.tick();
            self.drain_updates();
            if self.dirty {
                self.rebuild();
            }
            self.flush_to(writer)?;
        }
        Ok(())
    }

    // --- Step API ---

    /// Mutate application state directly. Marks dirty for rebuild.
    pub fn update(&mut self, f: impl FnOnce(&mut S)) {
        f(&mut self.state);
        self.dirty = true;
    }

    /// Forward a terminal event to the component tree (focus routing).
    pub fn handle_event(&mut self, event: &Event) {
        self.inline.handle_event(event);
    }

    /// Advance active effects (animations, intervals).
    pub fn tick(&mut self) {
        self.inline.tick();
    }

    /// Whether any effects are active (e.g., spinner animation).
    pub fn has_active(&self) -> bool {
        self.inline.has_active()
    }

    /// Read-only access to the application state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Whether external code has requested exit via [`Handle::exit`].
    pub fn is_exit_requested(&self) -> bool {
        self.exit.load(Ordering::Acquire)
    }

    /// Drain pending handle updates, rebuild if dirty, render to writer.
    pub fn flush(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.drain_updates();
        if self.dirty {
            self.rebuild();
        }
        self.flush_to(writer)
    }

    /// Access the inner [`InlineRenderer`] for advanced use.
    pub fn renderer(&mut self) -> &mut InlineRenderer {
        &mut self.inline
    }

    // --- Internals ---

    fn event_loop(
        &mut self,
        handler: &mut impl FnMut(&Event, &mut S) -> ControlFlow,
        stdout: &mut impl Write,
    ) -> io::Result<()> {
        loop {
            if self.exit.load(Ordering::Acquire) {
                break;
            }

            let poll_timeout = if self.inline.has_active() {
                Duration::from_millis(16) // ~60fps when animating
            } else {
                Duration::from_millis(100) // idle
            };

            if event::poll(poll_timeout)? {
                let evt = event::read()?;

                // Ctrl+C always exits
                if let Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    kind: KeyEventKind::Press,
                    ..
                }) = &evt
                {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }
                }

                // Handle resize
                if let Event::Resize(new_width, _) = &evt {
                    let output = self.inline.resize(*new_width);
                    stdout.write_all(&output)?;
                    stdout.flush()?;
                    // Rebuild at new width
                    self.dirty = true;
                    // Fall through to rebuild+render below
                } else {
                    // Framework handles first (focus routing, tab cycling)
                    self.inline.handle_event(&evt);

                    // Then app handler
                    let flow = handler(&evt, &mut self.state);
                    self.dirty = true;

                    if matches!(flow, ControlFlow::Exit) {
                        break;
                    }
                }
            }

            // Drain handle updates
            self.drain_updates();

            // Tick effects
            if self.inline.has_active() {
                self.inline.tick();
            }

            // Rebuild + render
            if self.dirty {
                self.rebuild();
            }
            self.flush_to(stdout)?;
        }

        Ok(())
    }

    fn rebuild(&mut self) {
        let elements = (self.view_fn)(&self.state);
        self.inline.rebuild(self.container, elements);
        self.dirty = false;
    }

    fn drain_updates(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            update(&mut self.state);
            self.dirty = true;
        }
    }

    fn flush_to(&mut self, writer: &mut impl Write) -> io::Result<()> {
        let output = self.inline.render();
        if !output.is_empty() {
            writer.write_all(&output)?;
            writer.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{SpinnerEl, TextBlockEl};
    use ratatui_core::style::Style;

    fn text_view(state: &Vec<String>) -> Elements {
        let mut els = Elements::new();
        for line in state {
            els.add(TextBlockEl::new().line(line.as_str(), Style::default()));
        }
        els
    }

    #[test]
    fn initial_flush_renders_content() {
        let (mut app, _handle) = Application::builder()
            .state(vec!["hello".to_string()])
            .view(text_view)
            .width(20)
            .build()
            .unwrap();

        let mut output = Vec::new();
        app.flush(&mut output).unwrap();
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("hello"));
    }

    #[test]
    fn update_triggers_rebuild_on_flush() {
        let (mut app, _handle) = Application::builder()
            .state(vec!["before".to_string()])
            .view(text_view)
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert_eq!(app.state(), &vec!["before".to_string()]);

        app.update(|s| {
            s.clear();
            s.push("after".to_string());
        });

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert_eq!(app.state(), &vec!["after".to_string()]);
        // Flush should produce output (the diff for changed text)
        assert!(!buf.is_empty());
    }

    #[test]
    fn handle_update_applied_on_flush() {
        let (mut app, handle) = Application::builder()
            .state(vec!["initial".to_string()])
            .view(text_view)
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        handle.update(|s| s.push("from_handle".to_string()));

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("from_handle"));
    }

    #[test]
    fn handle_update_from_another_thread() {
        let (mut app, handle) = Application::builder()
            .state(0u32)
            .view(|n: &u32| {
                let mut els = Elements::new();
                els.add(TextBlockEl::new().line(
                    &format!("count: {}", n),
                    Style::default(),
                ));
                els
            })
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        let t = std::thread::spawn(move || {
            handle.update(|s| *s = 42);
        });
        t.join().unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert_eq!(*app.state(), 42);
    }

    #[test]
    fn state_readable() {
        let (app, _handle) = Application::builder()
            .state(42u32)
            .view(|_: &u32| Elements::new())
            .width(10)
            .build()
            .unwrap();

        assert_eq!(*app.state(), 42);
    }

    #[test]
    fn handle_exit_sets_flag() {
        let (app, handle) = Application::builder()
            .state(0u32)
            .view(|_: &u32| Elements::new())
            .width(10)
            .build()
            .unwrap();

        assert!(!app.is_exit_requested());
        handle.exit();
        assert!(app.is_exit_requested());
    }

    #[test]
    fn has_active_reflects_effects() {
        let (mut app, _handle) = Application::builder()
            .state(true) // show spinner
            .view(|show: &bool| {
                let mut els = Elements::new();
                if *show {
                    els.add(SpinnerEl::new("loading")).key("s");
                }
                els
            })
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert!(app.has_active());

        // Hide spinner
        app.update(|s| *s = false);
        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert!(!app.has_active());
    }

    #[test]
    fn tick_advances_effects() {
        let (mut app, _handle) = Application::builder()
            .state(true)
            .view(|show: &bool| {
                let mut els = Elements::new();
                if *show {
                    els.add(SpinnerEl::new("loading"));
                }
                els
            })
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // Wait for interval to elapse and tick
        std::thread::sleep(Duration::from_millis(100));
        app.tick();

        // Tick should have fired — render should produce output
        // (spinner frame changed → dirty → re-render)
        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        // Content should exist (spinner renders a frame character)
        assert!(!buf.is_empty());
    }

    #[test]
    fn multiple_handle_updates_batched() {
        let (mut app, handle) = Application::builder()
            .state(0u32)
            .view(|n: &u32| {
                let mut els = Elements::new();
                els.add(TextBlockEl::new().line(
                    &format!("count: {}", n),
                    Style::default(),
                ));
                els
            })
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // Send multiple updates
        handle.update(|s| *s += 1);
        handle.update(|s| *s += 1);
        handle.update(|s| *s += 1);

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        // All three applied in one flush
        assert_eq!(*app.state(), 3);
        // Flush should produce output (the diff for changed text)
        assert!(!buf.is_empty());
    }

    #[test]
    fn empty_state_produces_no_content() {
        let (mut app, _handle) = Application::builder()
            .state(())
            .view(|_: &()| Elements::new())
            .width(10)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        // Empty content — no escape sequences needed
        assert!(buf.is_empty());
    }

    #[test]
    fn renderer_accessible() {
        let (mut app, _handle) = Application::builder()
            .state(vec!["test".to_string()])
            .view(text_view)
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // Can access renderer for advanced operations
        let renderer = app.renderer();
        assert!(!renderer.has_active());
    }
}
