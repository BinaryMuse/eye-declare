use std::any::{Any, TypeId};
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Guard that restores terminal state on drop (including panic unwind).
struct RawModeGuard {
    bracketed_paste: bool,
    keyboard_enhanced: bool,
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        use crossterm::execute;
        let mut stdout = io::stdout();
        if self.keyboard_enhanced {
            let _ = execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
        }
        if self.bracketed_paste {
            let _ = execute!(stdout, crossterm::event::DisableBracketedPaste);
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = stdout.write_all(b"\x1b[?25h");
        let _ = stdout.flush();
    }
}

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};

use crate::component::{EventResult, TrackedRef, VStack};
use crate::element::Elements;
use crate::inline::InlineRenderer;
use crate::node::NodeId;

type StateUpdateFn<S> = Box<dyn FnOnce(&mut S) + Send>;
type TrackedStateUpdateFn<S> = Box<dyn FnOnce(&mut TrackedRef<'_, S>) + Send>;
type StateGetFn<S> = Box<dyn FnOnce(&S) + Send>;
type ViewFn<S> = Box<dyn Fn(&S) -> Elements>;
type CommitCallbackFn<S> = Box<dyn FnMut(&CommittedElement, &mut S)>;
type EventHandlerFn<'a, S> = Option<&'a mut dyn FnMut(&Event, &mut S) -> ControlFlow>;

enum AppMessage<S> {
    UpdateState(StateUpdateFn<S>),
    UpdateStateTracked(TrackedStateUpdateFn<S>),
    GetState(StateGetFn<S>),
}

/// Information about an element that has scrolled into terminal scrollback.
///
/// Passed to the [`ApplicationBuilder::on_commit`] callback. Use the `key`
/// field to identify which element was committed so you can evict the
/// corresponding data from your application state.
#[derive(Debug, Clone)]
pub struct CommittedElement {
    /// The element's explicit key, if one was set via `.key()` or the
    /// `key:` prop in the `element!` macro.
    pub key: Option<String>,
    /// The element's positional index among its siblings at the time of
    /// commit. Elements are always committed front-to-back, so this is 0
    /// for the first committed element in each batch.
    pub index: usize,
}

/// Controls whether the [`Application`] event loop continues or stops.
///
/// Returned from the event handler passed to
/// [`Application::run_interactive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    /// Keep the event loop running.
    Continue,
    /// Stop the event loop and return from `run_interactive`.
    Exit,
}

/// How the framework handles Ctrl+C in interactive modes.
///
/// Set via [`ApplicationBuilder::ctrl_c`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CtrlCBehavior {
    /// Exit the event loop immediately (default).
    ///
    /// Ctrl+C is intercepted before reaching any component.
    #[default]
    Exit,
    /// Deliver Ctrl+C to the component tree like any other key event.
    ///
    /// The application is responsible for exiting via [`Handle::exit`]
    /// or [`ControlFlow::Exit`]. This enables patterns like "press
    /// Ctrl+C again to exit" confirmations.
    Deliver,
}

/// Which keyboard protocol to use in interactive modes.
///
/// Set via [`ApplicationBuilder::keyboard_protocol`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardProtocol {
    /// Standard terminal key reporting (default).
    ///
    /// Compatible with all terminals. Some key combinations are
    /// ambiguous (e.g., Tab vs Ctrl+I, Enter vs Ctrl+M).
    #[default]
    Legacy,
    /// Enable the [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
    /// if the terminal supports it, falling back to legacy silently.
    ///
    /// Provides key disambiguation, modifier reporting, and
    /// key release events in supporting terminals (kitty,
    /// WezTerm, foot, Windows Terminal, etc.).
    Enhanced,
}

/// A thread-safe handle for sending state updates to a running [`Application`].
///
/// `Handle` is `Clone + Send + Sync`. Clone it freely and move clones into
/// threads or async tasks. Updates are batched — multiple calls to
/// [`update`](Handle::update) between frames are applied together before
/// the next rebuild.
///
/// When all `Handle` clones are dropped and no lifecycle effects remain
/// active, [`Application::run`] exits automatically.
///
/// ```ignore
/// let h = handle.clone();
/// tokio::spawn(async move {
///     h.update(|state| state.response.push_str(&token));
/// });
/// ```
pub struct Handle<S: Send + 'static> {
    tx: mpsc::UnboundedSender<AppMessage<S>>,
    exit: Arc<AtomicBool>,
}

impl<S: Send + 'static> Handle<S> {
    /// Queue a state mutation. Applied on the next frame.
    ///
    /// This is non-blocking and can be called from both sync and
    /// async contexts. Always triggers a rebuild.
    pub fn update(&self, f: impl FnOnce(&mut S) + Send + 'static) {
        let _ = self.tx.send(AppMessage::UpdateState(Box::new(f)));
    }

    /// Queue a state mutation with dirty tracking. Only triggers a
    /// rebuild if the callback actually writes through `DerefMut`.
    ///
    /// Use `state.read()` for reads that shouldn't trigger a rebuild,
    /// and direct field access (which goes through `DerefMut`) for writes.
    pub fn update_tracked(&self, f: impl FnOnce(&mut TrackedRef<'_, S>) + Send + 'static) {
        let _ = self.tx.send(AppMessage::UpdateStateTracked(Box::new(f)));
    }

    /// Get the current state.
    pub fn fetch<T: Send + 'static>(
        &self,
        f: impl FnOnce(&S) -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(AppMessage::GetState(Box::new(move |s| {
            let _ = tx.send(f(s));
        })));

        rx
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
///
/// Created via [`Application::builder`]. Requires at minimum a
/// [`state`](ApplicationBuilder::state) and a [`view`](ApplicationBuilder::view)
/// function before calling [`build`](ApplicationBuilder::build).
pub struct ApplicationBuilder<S: Send + 'static> {
    state: Option<S>,
    view_fn: Option<ViewFn<S>>,
    width: Option<u16>,
    on_commit: Option<CommitCallbackFn<S>>,
    root_contexts: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
    ctrl_c: CtrlCBehavior,
    keyboard_protocol: KeyboardProtocol,
    extra_newlines_at_exit: usize,
    bracketed_paste: bool,
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

    /// Set a callback for when elements scroll into terminal scrollback.
    ///
    /// The callback receives a [`CommittedElement`] identifying which
    /// element was committed, and a mutable reference to the app state.
    /// Use this to evict committed data from state so the view function
    /// produces fewer elements on the next rebuild.
    ///
    /// ```ignore
    /// .on_commit(|committed, state| {
    ///     state.messages.remove(0); // evict from front
    /// })
    /// ```
    pub fn on_commit(mut self, f: impl FnMut(&CommittedElement, &mut S) + 'static) -> Self {
        self.on_commit = Some(Box::new(f));
        self
    }

    /// Set how Ctrl+C is handled in interactive modes.
    ///
    /// Default: [`CtrlCBehavior::Exit`] — Ctrl+C exits immediately.
    ///
    /// Set to [`CtrlCBehavior::Deliver`] to route Ctrl+C through the
    /// component tree, enabling custom handling (e.g., confirmation
    /// dialogs or "press again to exit" patterns).
    ///
    /// Has no effect on [`Application::run`] (non-interactive mode).
    pub fn ctrl_c(mut self, behavior: CtrlCBehavior) -> Self {
        self.ctrl_c = behavior;
        self
    }

    /// Set the keyboard protocol for interactive modes.
    ///
    /// Default: [`KeyboardProtocol::Legacy`] — standard key reporting.
    ///
    /// [`KeyboardProtocol::Enhanced`] enables the kitty keyboard
    /// protocol for key disambiguation and modifier reporting in
    /// supporting terminals. Falls back to legacy silently if the
    /// terminal does not support it.
    ///
    /// Has no effect on [`Application::run`] (non-interactive mode).
    pub fn keyboard_protocol(mut self, protocol: KeyboardProtocol) -> Self {
        self.keyboard_protocol = protocol;
        self
    }

    /// Enable bracketed paste mode.
    ///
    /// When enabled, pasted text is delivered as a single
    /// [`Event::Paste(String)`](crossterm::event::Event::Paste) instead
    /// of individual key events. This lets text input components
    /// distinguish typed input from pasted content.
    ///
    /// Default: `false`. Safe to enable — terminals that don't support
    /// it silently ignore the escape sequence.
    ///
    /// Has no effect on [`Application::run`] (non-interactive mode).
    pub fn bracketed_paste(mut self, enabled: bool) -> Self {
        self.bracketed_paste = enabled;
        self
    }

    /// Register a root-level context value available to all components.
    ///
    /// Components access root contexts via
    /// [`Hooks::use_context`](crate::Hooks::use_context) in their
    /// lifecycle method. Root contexts persist for the lifetime of
    /// the application.
    ///
    /// Call multiple times for different types. Calling twice with
    /// the same type replaces the previous value.
    ///
    /// ```ignore
    /// let (mut app, handle) = Application::builder()
    ///     .state(MyState::default())
    ///     .view(my_view)
    ///     .with_context(event_sender)
    ///     .with_context(AppConfig { theme: "dark".into() })
    ///     .build()?;
    /// ```
    pub fn with_context<T: Any + Send + Sync>(mut self, value: T) -> Self {
        self.root_contexts
            .push((TypeId::of::<T>(), Box::new(value)));
        self
    }

    /// Set the number of extra newlines to add at exit.
    ///
    /// This is useful for applications that need to add extra newlines at exit
    /// to ensure that the last line of output is visible, for example,
    /// when using a prompt that uses cursor manipulation (e.g.,
    /// Powerlevel10k's PROMPT_SP or instant prompt) which can
    /// overwrite the line immediately above the prompt.
    ///
    /// Default: 0.
    pub fn extra_newlines_at_exit(mut self, extra_newlines: usize) -> Self {
        self.extra_newlines_at_exit = extra_newlines;
        self
    }

    /// Build the Application and its Handle.
    ///
    /// Queries terminal size if width was not specified, which may
    /// fail if no terminal is attached.
    pub fn build(self) -> io::Result<(Application<S>, Handle<S>)> {
        let state = self.state.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Application requires .state()")
        })?;
        let view_fn = self.view_fn.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Application requires .view()")
        })?;
        let width = match self.width {
            Some(w) => w,
            None => crossterm::terminal::size()?.0,
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let exit = Arc::new(AtomicBool::new(false));
        let handle = Handle {
            tx,
            exit: exit.clone(),
        };

        let mut inline = InlineRenderer::new(width);
        for (type_id, value) in self.root_contexts {
            inline.set_root_context_raw(type_id, value);
        }
        let container = inline.push(VStack);

        let app = Application {
            state,
            view_fn,
            inline,
            container,
            dirty: true,
            on_commit: self.on_commit,
            rx,
            exit,
            ctrl_c: self.ctrl_c,
            keyboard_protocol: self.keyboard_protocol,
            extra_newlines_at_exit: self.extra_newlines_at_exit,
            bracketed_paste: self.bracketed_paste,
        };

        Ok((app, handle))
    }
}

/// The high-level application wrapper — owns state, a view function, and a renderer.
///
/// `Application` is the recommended entry point for most programs. It manages
/// the render loop, processes [`Handle`] updates, ticks lifecycle effects,
/// and handles terminal resize.
///
/// # Two usage modes
///
/// **Non-interactive** — state changes flow entirely through the [`Handle`]:
///
/// ```ignore
/// let (mut app, handle) = Application::builder()
///     .state(MyState::new())
///     .view(my_view)
///     .build()?;
///
/// tokio::spawn(async move {
///     handle.update(|s| s.done = true);
///     // handle dropped → app exits when effects stop
/// });
///
/// app.run().await?;
/// ```
///
/// **Interactive** — terminal raw mode with event handling:
///
/// ```ignore
/// app.run_interactive(|event, state| {
///     // handle keyboard/mouse events, mutate state
///     ControlFlow::Continue
/// }).await?;
/// ```
///
/// # Step API
///
/// For custom event loops or embedding, use the step methods directly:
///
/// ```ignore
/// app.update(|s| s.count += 1);
/// app.tick();
/// app.flush(&mut stdout)?;
/// ```
pub struct Application<S: Send + 'static> {
    state: S,
    view_fn: ViewFn<S>,
    inline: InlineRenderer,
    container: NodeId,
    dirty: bool,
    on_commit: Option<CommitCallbackFn<S>>,
    rx: mpsc::UnboundedReceiver<AppMessage<S>>,
    exit: Arc<AtomicBool>,
    ctrl_c: CtrlCBehavior,
    keyboard_protocol: KeyboardProtocol,
    extra_newlines_at_exit: usize,
    bracketed_paste: bool,
}

impl<S: Send + 'static> Application<S> {
    /// Create a new [`ApplicationBuilder`].
    pub fn builder() -> ApplicationBuilder<S> {
        ApplicationBuilder {
            state: None,
            view_fn: None,
            width: None,
            on_commit: None,
            root_contexts: Vec::new(),
            ctrl_c: CtrlCBehavior::default(),
            keyboard_protocol: KeyboardProtocol::default(),
            extra_newlines_at_exit: 0,
            bracketed_paste: false,
        }
    }

    /// Run the render loop.
    ///
    /// Processes handle updates, ticks active effects, and renders
    /// automatically. Does not poll terminal events or enable raw
    /// mode. Exits when all [`Handle`]s are dropped and no effects
    /// remain, or when [`Handle::exit`] is called.
    ///
    /// This is the primary entry point for non-interactive use.
    /// All state changes flow through the Handle:
    ///
    /// ```ignore
    /// let (mut app, handle) = Application::builder()
    ///     .state(MyState::new())
    ///     .view(my_view)
    ///     .build()?;
    ///
    /// tokio::spawn(async move {
    ///     handle.update(|s| s.message = "hello".into());
    ///     // handle dropped → app exits when effects stop
    /// });
    ///
    /// app.run().await?;
    /// ```
    pub async fn run(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        self.render_loop(&mut stdout).await
    }

    /// Run the interactive event loop with component-driven event handling.
    ///
    /// Enables terminal raw mode and processes terminal events
    /// (keyboard, mouse, resize) through the component tree's
    /// two-phase event dispatch (capture then bubble),
    /// including focus management and Tab cycling. Unlike
    /// [`run_interactive`](Application::run_interactive), raw terminal
    /// events are **not** exposed to the caller.
    ///
    /// Use this when components handle their own events and translate
    /// them into app-domain actions via channels or other mechanisms
    /// passed through the [context system](crate::Hooks::provide_context).
    ///
    /// Exits when [`Handle::exit`] is called, or when all Handles are
    /// dropped and no effects remain active. Ctrl+C exits by default
    /// (configurable via [`ApplicationBuilder::ctrl_c`]).
    ///
    /// ```ignore
    /// let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    /// let (mut app, handle) = Application::builder()
    ///     .state(MyState::default())
    ///     .view(my_view)
    ///     .with_context(tx)
    ///     .build()?;
    ///
    /// // App event loop in a separate task
    /// let h = handle.clone();
    /// tokio::spawn(async move {
    ///     while let Some(event) = rx.recv().await {
    ///         match event {
    ///             AppEvent::Submit(val) => h.update(|s| s.result = val),
    ///             AppEvent::Quit => { h.exit(); break; }
    ///         }
    ///     }
    /// });
    ///
    /// app.run_loop().await?;
    /// ```
    pub async fn run_loop(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();

        // Initial build + render before entering raw mode
        self.rebuild();
        self.flush_to(&mut stdout)?;

        let guard = self.enter_raw_mode()?;
        let result = self.interactive_loop(None, &mut stdout).await;
        self.leave_raw_mode(guard, &mut stdout);

        result
    }

    /// Run the interactive event loop with a raw event handler.
    ///
    /// Enables terminal raw mode and uses `tokio::select!` to
    /// multiplex terminal events, handle updates, and effect ticks.
    /// The handler receives terminal events and mutable state;
    /// return [`ControlFlow::Exit`] to stop. Ctrl+C exits by default
    /// (configurable via [`ApplicationBuilder::ctrl_c`]).
    ///
    /// For most applications, prefer [`run_loop`](Application::run_loop)
    /// with the context system. Use this when you need direct access
    /// to raw terminal events.
    pub async fn run_interactive(
        &mut self,
        mut handler: impl FnMut(&Event, &mut S) -> ControlFlow,
    ) -> io::Result<()> {
        let mut stdout = io::stdout();

        // Initial build + render before entering raw mode
        self.rebuild();
        self.flush_to(&mut stdout)?;

        let guard = self.enter_raw_mode()?;
        let result = self.interactive_loop(Some(&mut handler), &mut stdout).await;
        self.leave_raw_mode(guard, &mut stdout);

        result
    }

    // --- Step API ---

    /// Mutate application state directly. Marks dirty for rebuild.
    pub fn update(&mut self, f: impl FnOnce(&mut S)) {
        f(&mut self.state);
        self.dirty = true;
    }

    /// Forward a terminal event to the component tree (focus routing).
    ///
    /// Returns [`EventResult::Consumed`] when a component in the tree
    /// handled the event, or [`EventResult::Ignored`] when no component
    /// claimed it.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        self.inline.handle_event(event)
    }

    /// Advance active effects (animations, intervals).
    pub fn tick(&mut self) {
        if self.inline.tick() {
            self.dirty = true;
        }
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
    /// Also checks for committed scrollback if `on_commit` is set.
    pub fn flush(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.drain_updates();
        if self.dirty {
            self.rebuild();
        }
        self.flush_to(writer)?;
        self.check_commits();
        Ok(())
    }

    /// Access the inner [`InlineRenderer`] for advanced use.
    pub fn renderer(&mut self) -> &mut InlineRenderer {
        &mut self.inline
    }

    // --- Internals ---

    /// Enable raw mode and any configured terminal protocols.
    /// Returns a guard that restores everything on drop.
    fn enter_raw_mode(&self) -> io::Result<RawModeGuard> {
        use crossterm::execute;
        let mut stdout = io::stdout();

        crossterm::terminal::enable_raw_mode()?;

        let bracketed_paste = self.bracketed_paste;
        if bracketed_paste {
            let _ = execute!(stdout, crossterm::event::EnableBracketedPaste);
        }

        let keyboard_enhanced = self.keyboard_protocol == KeyboardProtocol::Enhanced && {
            // Only push if the terminal supports it
            crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        };
        if keyboard_enhanced {
            let _ = execute!(
                stdout,
                crossterm::event::PushKeyboardEnhancementFlags(
                    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            );
        }

        Ok(RawModeGuard {
            bracketed_paste,
            keyboard_enhanced,
        })
    }

    /// Finalize after an interactive loop: reclaim blank rows, restore terminal.
    ///
    /// Best-effort: I/O errors during teardown are silently ignored so
    /// the original loop result is never shadowed.
    fn leave_raw_mode(&mut self, guard: RawModeGuard, stdout: &mut impl Write) {
        // Write finalize bytes while still in raw mode — cooked mode's
        // output processing (OPOST) can interfere with escape sequences.
        let finalize_bytes = self.inline.finalize();
        if !finalize_bytes.is_empty() {
            let _ = stdout.write_all(&finalize_bytes);
            let _ = stdout.flush();
        }
        // Now drop guard: disables protocols, raw mode, restores cursor.
        drop(guard);
        // Two newlines: one to end the content region, one as a buffer
        // for shell prompts that use cursor manipulation (e.g.,
        // Powerlevel10k's PROMPT_SP or instant prompt) which can
        // overwrite the line immediately above the prompt.
        let _ = writeln!(stdout);
        for _ in 0..self.extra_newlines_at_exit {
            let _ = writeln!(stdout);
        }
        let _ = stdout.flush();
    }

    async fn render_loop(&mut self, writer: &mut impl Write) -> io::Result<()> {
        // Initial build + render
        self.rebuild();
        self.flush_to(writer)?;

        let mut tick_interval = tokio::time::interval(Duration::from_millis(16));
        let mut channel_open = true;

        loop {
            if self.exit.load(Ordering::Acquire) {
                break;
            }

            let has_active = self.inline.has_active();

            // Exit when channel closed and no effects remain
            if !channel_open && !has_active {
                break;
            }

            tokio::select! {
                result = self.rx.recv(), if channel_open => {
                    match result {
                        Some(AppMessage::GetState(get)) => {
                            get(&self.state);
                        }
                        Some(AppMessage::UpdateState(update)) => {
                            update(&mut self.state);
                            self.dirty = true;
                        }
                        Some(AppMessage::UpdateStateTracked(update)) => {
                            self.apply_tracked_update(update);
                        }
                        None => {
                            // All Handles dropped
                            channel_open = false;
                        }
                    }
                }
                _ = tick_interval.tick(), if has_active => {
                    if self.inline.tick() {
                        self.dirty = true;
                    }
                }
            }

            if self.dirty {
                self.rebuild();
            }
            self.flush_to(writer)?;
            self.check_commits();
        }

        // Drain any pending messages so that callers blocked on
        // handle.fetch() are unblocked with the final state, rather than
        // having their oneshot senders silently dropped.
        self.drain_updates();

        // Final flush + commit check + reclaim trailing blank rows
        if self.dirty {
            self.rebuild();
        }
        self.flush_to(writer)?;
        self.check_commits();
        let finalize_bytes = self.inline.finalize();
        if !finalize_bytes.is_empty() {
            writer.write_all(&finalize_bytes)?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Unified interactive event loop. Handles terminal events, Handle
    /// updates, and effect ticks. If `handler` is `Some`, raw events are
    /// forwarded to it after component dispatch; if `None`, events are
    /// only routed through the component tree.
    ///
    /// Exits when:
    /// - `Handle::exit` is called
    /// - `handler` returns `ControlFlow::Exit`
    /// - All Handles are dropped and no effects remain active
    /// - Ctrl+C is pressed (when `ctrl_c` is `CtrlCBehavior::Exit`)
    async fn interactive_loop(
        &mut self,
        mut handler: EventHandlerFn<'_, S>,
        stdout: &mut impl Write,
    ) -> io::Result<()> {
        let mut event_stream = crossterm::event::EventStream::new();
        let mut tick_interval = tokio::time::interval(Duration::from_millis(16));
        let mut channel_open = true;

        loop {
            if self.exit.load(Ordering::Acquire) {
                break;
            }

            let has_active = self.inline.has_active();

            // Exit when channel closed and no effects remain
            if !channel_open && !has_active {
                break;
            }

            tokio::select! {
                maybe_event = event_stream.next() => {
                    let evt = match maybe_event {
                        Some(Ok(evt)) => evt,
                        Some(Err(e)) => return Err(e),
                        None => break, // stream ended
                    };

                    // Ctrl+C handling
                    if self.ctrl_c == CtrlCBehavior::Exit
                        && let Event::Key(KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers,
                            kind: KeyEventKind::Press,
                            ..
                        }) = &evt
                        && modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    // Handle resize
                    if let Event::Resize(new_width, _) = &evt {
                        let output = self.inline.resize(*new_width);
                        stdout.write_all(&output)?;
                        stdout.flush()?;
                        self.dirty = true;
                    } else {
                        // Framework handles first (focus routing, tab cycling)
                        let result = self.inline.handle_event(&evt);
                        self.dirty = true;

                        // Then app handler, only if the component tree didn't consume the event
                        if result == EventResult::Ignored
                            && let Some(ref mut h) = handler {
                                let flow = h(&evt, &mut self.state);
                                if matches!(flow, ControlFlow::Exit) {
                                    break;
                                }
                            }
                    }
                }

                result = self.rx.recv(), if channel_open => {
                    match result {
                        Some(AppMessage::UpdateState(update)) => {
                            update(&mut self.state);
                            self.dirty = true;
                        }
                        Some(AppMessage::UpdateStateTracked(update)) => {
                            self.apply_tracked_update(update);
                        }
                        Some(AppMessage::GetState(get)) => {
                            get(&self.state);
                        }
                        None => {
                            // All Handles dropped
                            channel_open = false;
                        }
                    }
                }

                _ = tick_interval.tick(), if has_active => {
                    if self.inline.tick() {
                        self.dirty = true;
                    }
                }
            }

            // Rebuild + render
            if self.dirty {
                self.rebuild();
            }
            self.flush_to(stdout)?;
            self.check_commits();
        }

        // Drain any pending messages so that callers blocked on
        // handle.fetch() are unblocked with the final state, rather than
        // having their oneshot senders silently dropped.
        self.drain_updates();

        // Final rebuild + render so state changes from the exit handler are visible
        if self.dirty {
            self.rebuild();
        }
        self.flush_to(stdout)?;
        self.check_commits();

        Ok(())
    }

    fn rebuild(&mut self) {
        let elements = (self.view_fn)(&self.state);
        self.inline.rebuild(self.container, elements);
        self.dirty = false;
    }

    fn apply_tracked_update(&mut self, update: TrackedStateUpdateFn<S>) {
        let mut tracked = TrackedRef::new(&mut self.state);
        update(&mut tracked);
        if tracked.is_dirty() {
            self.dirty = true;
        }
    }

    fn drain_updates(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                AppMessage::UpdateState(update) => {
                    update(&mut self.state);
                    self.dirty = true;
                }
                AppMessage::UpdateStateTracked(update) => {
                    self.apply_tracked_update(update);
                }
                AppMessage::GetState(get) => {
                    get(&self.state);
                }
            }
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

    fn check_commits(&mut self) {
        let terminal_height = crossterm::terminal::size()
            .map(|(_, h)| h)
            .unwrap_or(u16::MAX);
        self.check_commits_with_height(terminal_height);
    }

    fn check_commits_with_height(&mut self, terminal_height: u16) {
        if self.on_commit.is_none() {
            return;
        }

        let committed = self
            .inline
            .detect_committed(self.container, terminal_height);
        if committed.is_empty() {
            return;
        }

        // Calculate total committed height
        let children = self.inline.children(self.container);
        let mut committed_height: u16 = 0;
        for &(i, _) in &committed {
            committed_height += self.inline.node_last_height(children[i]);
        }

        // Fire callbacks
        let on_commit = self.on_commit.as_mut().unwrap();
        for (index, key) in &committed {
            let elem = CommittedElement {
                key: key.clone(),
                index: *index,
            };
            on_commit(&elem, &mut self.state);
        }
        self.dirty = true;

        // Drop committed nodes and adjust frame tracking
        self.inline
            .commit(self.container, committed.len(), committed_height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::spinner::Spinner;
    use crate::components::text::Text;

    fn text_view(state: &Vec<String>) -> Elements {
        let mut els = Elements::new();
        for line in state {
            els.add(Text::unstyled(line.as_str()));
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
                els.add(Text::unstyled(format!("count: {}", n)));
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
                    els.add(Spinner::new("loading")).key("s");
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
                    els.add(Spinner::new("loading"));
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
                els.add(Text::unstyled(format!("count: {}", n)));
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

    #[tokio::test]
    async fn run_exits_when_handle_dropped_and_idle() {
        let (mut app, handle) = Application::builder()
            .state(true) // show spinner
            .view(|show: &bool| {
                let mut els = Elements::new();
                if *show {
                    els.add(Spinner::new("loading")).key("s");
                }
                els
            })
            .width(20)
            .build()
            .unwrap();

        // Stop the spinner after a short delay, then drop handle
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            handle.update(|s| *s = false);
            // handle dropped here
        });

        let mut buf = Vec::new();
        app.render_loop(&mut buf).await.unwrap();
        assert!(!app.has_active());
    }

    #[tokio::test]
    async fn handle_update_from_async_task() {
        let (mut app, handle) = Application::builder()
            .state(0u32)
            .view(|n: &u32| {
                let mut els = Elements::new();
                els.add(Text::unstyled(format!("count: {}", n)));
                els
            })
            .width(20)
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        let task = tokio::spawn(async move {
            handle.update(|s| *s = 99);
        });
        task.await.unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        assert_eq!(*app.state(), 99);
    }

    // --- Committed scrollback tests ---

    #[test]
    fn commit_fires_when_content_exceeds_terminal() {
        use std::sync::{Arc, Mutex};

        let committed_keys: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let keys_clone = committed_keys.clone();

        let (mut app, _handle) = Application::builder()
            .state(vec![
                "line1".to_string(),
                "line2".to_string(),
                "line3".to_string(),
            ])
            .view(|lines: &Vec<String>| {
                let mut els = Elements::new();
                for (i, line) in lines.iter().enumerate() {
                    els.add(Text::unstyled(line.as_str()))
                        .key(format!("line-{}", i));
                }
                els
            })
            .width(20)
            .on_commit(move |elem, state| {
                keys_clone.lock().unwrap().push(elem.key.clone());
                state.remove(0);
            })
            .build()
            .unwrap();

        // Render: 3 lines, each 1 row tall
        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // emitted_rows is now 3. Simulate terminal height of 2:
        // scrollback = 3 - 2 = 1, so the first child (1 row) is committed
        app.check_commits_with_height(2);

        let keys = committed_keys.lock().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], Some("line-0".to_string()));
        drop(keys);

        // State should have been mutated (first element removed)
        assert_eq!(app.state().len(), 2);
        assert_eq!(app.state()[0], "line2");
    }

    #[test]
    fn no_commit_when_all_content_visible() {
        let committed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = committed_count.clone();

        let (mut app, _handle) = Application::builder()
            .state(vec!["line1".to_string()])
            .view(|lines: &Vec<String>| {
                let mut els = Elements::new();
                for line in lines {
                    els.add(Text::unstyled(line.as_str()));
                }
                els
            })
            .width(20)
            .on_commit(move |_, _| {
                count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // Terminal height 10, emitted 1 row — nothing in scrollback
        app.check_commits_with_height(10);
        assert_eq!(
            committed_count.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn no_commit_without_callback() {
        let (mut app, _handle) = Application::builder()
            .state(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .view(|lines: &Vec<String>| {
                let mut els = Elements::new();
                for line in lines {
                    els.add(Text::unstyled(line.as_str()));
                }
                els
            })
            .width(20)
            // No on_commit callback
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // Should not panic or commit anything
        app.check_commits_with_height(1);
        assert_eq!(app.state().len(), 3); // unchanged
    }

    #[test]
    fn multiple_commits_at_once() {
        let committed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = committed_count.clone();

        let (mut app, _handle) = Application::builder()
            .state(vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
            ])
            .view(|lines: &Vec<String>| {
                let mut els = Elements::new();
                for line in lines {
                    els.add(Text::unstyled(line.as_str()));
                }
                els
            })
            .width(20)
            .on_commit(move |_, state: &mut Vec<String>| {
                count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                state.remove(0);
            })
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();

        // 5 rows emitted, terminal height 2 → 3 rows in scrollback → 3 commits
        app.check_commits_with_height(2);
        assert_eq!(
            committed_count.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
        assert_eq!(app.state().len(), 2);
        assert_eq!(app.state()[0], "d");
    }

    // --- Context tests ---

    /// Component that reads a String from context into its state.
    struct CtxReader;

    #[derive(Default)]
    struct CtxReaderState {
        value: Option<String>,
    }

    impl crate::component::Component for CtxReader {
        type State = CtxReaderState;

        fn render(
            &self,
            area: ratatui_core::layout::Rect,
            buf: &mut ratatui_core::buffer::Buffer,
            state: &Self::State,
        ) {
            use ratatui_core::widgets::Widget;
            if let Some(ref v) = state.value {
                ratatui_widgets::paragraph::Paragraph::new(v.as_str()).render(area, buf);
            }
        }

        fn lifecycle(
            &self,
            hooks: &mut crate::hooks::Hooks<Self, Self::State>,
            _state: &Self::State,
        ) {
            hooks.use_context::<String>(|value, _props, state| {
                state.value = value.cloned();
            });
        }
    }

    #[test]
    fn with_context_available_to_components() {
        let (mut app, _handle) = Application::builder()
            .state(())
            .view(|_: &()| {
                let mut els = Elements::new();
                els.add(CtxReader);
                els
            })
            .width(30)
            .with_context("app-context".to_string())
            .build()
            .unwrap();

        let mut buf = Vec::new();
        app.flush(&mut buf).unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("app-context"));
    }

    #[tokio::test]
    async fn fetch_succeeds_after_exit_called_inside_update() {
        // Regression test: handle.fetch() enqueued after handle.exit() is
        // called inside a handle.update() closure must still be processed.
        // Previously, the loop would break immediately after processing the
        // update that called exit(), leaving the fetch's oneshot sender
        // unresolved and blocking_recv() returning Err.
        let (mut app, handle) = Application::builder()
            .state(0u32)
            .view(|n: &u32| {
                let mut els = Elements::new();
                els.add(Text::unstyled(format!("n={}", n)));
                els
            })
            .width(20)
            .build()
            .unwrap();

        // Simulate the dispatch pattern: update+exit, then fetch, from a
        // blocking thread — exactly what Atuin's dispatch loop does.
        let fetch_handle = tokio::task::spawn_blocking(move || {
            // Step 1: enqueue state mutation + exit (like on_exit does)
            let h3 = handle.clone();
            handle.update(move |s| {
                *s = 42;
                h3.exit();
            });

            // Step 2: immediately fetch state (like persist_session does)
            handle.fetch(|s| *s).blocking_recv()
        });

        let mut buf = Vec::new();
        app.render_loop(&mut buf).await.unwrap();

        // The fetch must have succeeded with the updated value
        let fetched = fetch_handle.await.unwrap();
        assert_eq!(fetched, Ok(42));
    }
}
