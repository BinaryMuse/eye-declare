# AGENTS.md
This file provides guidance to AI coding assistants working in this repository.

**Note:** CLAUDE.md, .clinerules, .cursorrules, .windsurfrules, .replit.md, GEMINI.md, .github/copilot-instructions.md, and .idx/airules.md are symlinks to AGENTS.md.

# eye-declare

Declarative inline TUI rendering library for Rust, built on Ratatui. Provides a React-like component model for terminal UIs that render inline into terminal scrollback (not full-screen). Designed for CLI tools, AI assistants, and interactive prompts.

## Crate Structure

```
crates/
  eye_declare/         Main library (ratatui-core, crossterm, tokio)
  eye_declare_macros/  element! proc macro (syn, quote, proc-macro2)
```

Workspace root `Cargo.toml` defines both members. The macro crate is behind the `macros` feature flag (enabled by default).

## Build & Commands

```sh
# Build
cargo build                          # Build all workspace crates
cargo build -p eye_declare           # Build main library only
cargo build -p eye_declare_macros    # Build proc macro only

# Test
cargo test                           # Run all tests
cargo test -p eye_declare            # Tests for main library
cargo test -p eye_declare_macros     # Tests for proc macro
cargo test test_name                 # Run a specific test by name

# Check / Lint
cargo check                          # Type check without building
cargo clippy                         # Lint with clippy

# Examples (all in crates/eye_declare/examples/)
cargo run --example chat             # Interactive chat with streaming
cargo run --example app              # Application wrapper with Handle updates
cargo run --example declarative      # View function pattern with element! macro
cargo run --example lifecycle        # Mount/unmount lifecycle hooks
cargo run --example interactive      # Focus, Tab cycling, text input
cargo run --example terminal_demo    # Sync imperative API with InlineRenderer
cargo run --example agent_sim        # Multi-component agent simulation
cargo run --example markdown_demo    # Markdown rendering showcase
cargo run --example growing          # Dynamically growing content
cargo run --example nested           # Nested component trees
cargo run --example wrapping         # Word wrapping and resize behavior

# Docs
cargo doc --open                     # Build and open API docs
```

## Code Style

- **Edition**: Rust 2024
- **Formatting**: Standard `rustfmt` conventions
- **Naming**: Snake case for functions/variables, PascalCase for types/components/traits
- **Imports**: Group by std, external crates, then internal modules
- **Types**: Use associated types on traits (e.g., `Component::State`). Prefer concrete types over trait objects where possible
- **Error handling**: Return `std::io::Result` from application-level functions. Use `Option` for absent values. Avoid `unwrap()` in library code
- **Components**: Implement the `Component` trait. Props are struct fields on `&self` (immutable). Internal state goes in the associated `State` type with `Tracked<T>` for automatic dirty detection
- **Proc macro code** (in `eye_declare_macros`): Parsing in `parse.rs`, code generation in `codegen.rs`, entry point in `lib.rs`
- **Feature flags**: The `macros` feature (default) gates `eye_declare_macros` dependency

## Testing

- **Framework**: Built-in Rust test framework (`#[test]`, `#[cfg(test)]`)
- **Test locations**: Inline `#[cfg(test)] mod tests` blocks in source files, plus `crates/eye_declare/tests/` for integration tests
- **Test files**: `crates/eye_declare/tests/element_macro.rs` (integration tests for the proc macro)
- **Run a single test**: `cargo test test_name` or `cargo test -p eye_declare test_name`
- **When tests fail, fix the code, not the test**

## Architecture

```
Application        State + view function + async event loop (tokio)
  InlineRenderer   Rendering, reconciliation, layout, diffing, scrollback
    ratatui-core   Buffer, Cell, Style, Widget primitives
    crossterm      Terminal I/O, event types
```

Key source files in `crates/eye_declare/src/`:
- `app.rs` — Application builder, run loop, Handle for cross-thread updates
- `component.rs` — Component trait definition
- `element.rs` — Element descriptors for the virtual tree
- `node.rs` — Reconciled node tree
- `inline.rs` — InlineRenderer (diff-based inline rendering)
- `renderer.rs` — Low-level rendering and buffer management
- `frame.rs` — Frame diffing and ANSI output
- `hooks.rs` — Lifecycle hooks (use_interval, use_mount, use_unmount, etc.)
- `context.rs` — Context system for ancestor-to-descendant data passing
- `children.rs` — Children/slot handling
- `components/` — Built-in components (TextBlock, Spinner, Markdown, VStack, HStack)
- `wrap.rs` — Word wrapping
- `escape.rs` — ANSI escape sequence handling

## Configuration

- No environment variables required
- No config files beyond `Cargo.toml`
- Rust stable toolchain
- tokio runtime required for async Application API

## CI/CD

- GitHub Actions workflow for docs deployment (`.github/workflows/docs.yml`)
- Docs built with [undox](https://github.com/BinaryMuse/undox) and deployed to GitHub Pages
- Doc content lives in `docs/content/`

## Commits

- Use conventional commit format for easy changelog generation
- Run `cargo fmt` and `cargo clippy -- -D warnings` before a permanent commit
- Be very careful not to push `main` - always work on a branch and open PRs
