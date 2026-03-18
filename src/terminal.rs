use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::component::{Component, Tracked};
use crate::inline::InlineRenderer;
use crate::node::NodeId;

/// Convenience wrapper that runs an eye_declare UI with a crossterm event loop.
///
/// Handles terminal raw mode, event polling, rendering, resize, and cleanup.
/// For standalone use outside of a PTY proxy like Atuin Hex.
///
/// ```no_run
/// use eye_delcare::{Terminal, TextBlock};
/// use ratatui_core::style::Style;
///
/// let mut term = Terminal::new().unwrap();
/// let id = term.push(TextBlock);
/// term.state_mut::<TextBlock>(id).push_unstyled("Hello!");
/// term.run(|event, renderer| {
///     // Return true to exit
///     false
/// }).unwrap();
/// ```
pub struct Terminal {
    renderer: InlineRenderer,
}

impl Terminal {
    /// Create a new Terminal with the current terminal width.
    pub fn new() -> io::Result<Self> {
        let (width, _) = crossterm::terminal::size()?;
        Ok(Self {
            renderer: InlineRenderer::new(width),
        })
    }

    /// Create with a specific width (useful for testing).
    pub fn with_width(width: u16) -> Self {
        Self {
            renderer: InlineRenderer::new(width),
        }
    }

    // --- Delegate tree/component methods to the inner renderer ---

    pub fn root(&self) -> NodeId {
        self.renderer.root()
    }

    pub fn append_child<C: Component>(&mut self, parent: NodeId, component: C) -> NodeId {
        self.renderer.append_child(parent, component)
    }

    pub fn push<C: Component>(&mut self, component: C) -> NodeId {
        self.renderer.push(component)
    }

    pub fn state_mut<C: Component>(&mut self, id: NodeId) -> &mut Tracked<C::State> {
        self.renderer.state_mut::<C>(id)
    }

    pub fn freeze(&mut self, id: NodeId) {
        self.renderer.freeze(id)
    }

    pub fn remove(&mut self, id: NodeId) {
        self.renderer.remove(id)
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.renderer.children(id)
    }

    pub fn set_focus(&mut self, id: NodeId) {
        self.renderer.set_focus(id)
    }

    pub fn clear_focus(&mut self) {
        self.renderer.clear_focus()
    }

    pub fn focus(&self) -> Option<NodeId> {
        self.renderer.focus()
    }

    /// Access the inner InlineRenderer for advanced use.
    pub fn renderer(&mut self) -> &mut InlineRenderer {
        &mut self.renderer
    }

    /// Render and flush to stdout.
    pub fn flush(&mut self) -> io::Result<()> {
        let output = self.renderer.render();
        if !output.is_empty() {
            let mut stdout = io::stdout();
            stdout.write_all(&output)?;
            stdout.flush()?;
        }
        Ok(())
    }

    /// Run the event loop.
    ///
    /// The `handler` callback receives each event and a mutable reference
    /// to the InlineRenderer. Return `true` from the handler to exit the
    /// loop. Ctrl+C always exits.
    ///
    /// Handles:
    /// - Raw mode enable/disable
    /// - Resize events (re-renders at new width)
    /// - Rendering after each event
    /// - Cleanup on exit
    pub fn run<F>(&mut self, mut handler: F) -> io::Result<()>
    where
        F: FnMut(&Event, &mut InlineRenderer) -> bool,
    {
        // Initial render before entering raw mode
        self.flush()?;

        crossterm::terminal::enable_raw_mode()?;

        let result = self.event_loop(&mut handler);

        crossterm::terminal::disable_raw_mode()?;
        // Show cursor in case it was hidden
        io::stdout().write_all(b"\x1b[?25h")?;
        println!();

        result
    }

    fn event_loop<F>(&mut self, handler: &mut F) -> io::Result<()>
    where
        F: FnMut(&Event, &mut InlineRenderer) -> bool,
    {
        let mut stdout = io::stdout();

        loop {
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

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
                let output = self.renderer.resize(*new_width);
                stdout.write_all(&output)?;
                stdout.flush()?;
                continue;
            }

            // Let the application handle the event
            let should_exit = handler(&evt, &mut self.renderer);
            if should_exit {
                break;
            }

            // Deliver to framework (Tab cycling, focus routing)
            self.renderer.handle_event(&evt);

            // Render
            let output = self.renderer.render();
            if !output.is_empty() {
                stdout.write_all(&output)?;
                stdout.flush()?;
            }
        }

        Ok(())
    }
}
