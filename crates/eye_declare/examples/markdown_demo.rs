use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use eye_declare::{InlineRenderer, Markdown, Spinner, Text, VStack};
use ratatui_core::style::{Color, Modifier, Style};

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // User prompt
    let _prompt = r.push(Text::styled(
        "› Explain how async/await works in Rust with an example",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    flush(&mut r, &mut stdout)?;

    // Spacer
    let _sp = r.push(Text::unstyled(""));

    // Response container
    let response = r.push(VStack);

    // Thinking spinner
    let think = r.append_child(response, Spinner::new("Thinking..."));
    animate_spinner(&mut r, &mut stdout, think, Duration::from_millis(1200))?;
    r.swap_component(think, Spinner::new("Thinking...").done("Thought for 1.2s"));
    flush(&mut r, &mut stdout)?;
    r.freeze(think);

    // Stream the markdown response by rebuilding with progressively more content
    let md_id = r.append_child(response, Markdown::new(""));

    let response_text = r#"## Async/Await in Rust

Rust's async/await is built on the **Future** trait. When you write an `async fn`, the compiler transforms it into a state machine that implements `Future`.

### Key Concepts

- **Futures are lazy** — they don't run until polled by an *executor*
- The `await` keyword yields control back to the executor
- An executor like `tokio` manages scheduling futures onto threads

### Example

```rust
async fn fetch_data(url: &str) -> Result<String, Error> {
    let response = reqwest::get(url).await?;
    let body = response.text().await?;
    Ok(body)
}

#[tokio::main]
async fn main() {
    let data = fetch_data("https://example.com").await;
    println!("Got: {:?}", data);
}
```

The `.await` points are where the runtime can **suspend** this task and run others. This is *cooperative* multitasking — tasks must explicitly yield via `await`."#;

    // Stream token by token
    let tokens: Vec<&str> = response_text
        .split_inclusive(|c: char| c.is_whitespace() || c == '\n')
        .collect();
    let mut accumulated = String::new();
    for token in &tokens {
        accumulated.push_str(token);
        r.swap_component(md_id, Markdown::new(accumulated.clone()));
        flush(&mut r, &mut stdout)?;
        thread::sleep(Duration::from_millis(20));
    }

    println!();
    Ok(())
}

fn flush(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
    let output = r.render();
    if !output.is_empty() {
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}

fn animate_spinner(
    r: &mut InlineRenderer,
    stdout: &mut impl Write,
    id: eye_declare::NodeId,
    duration: Duration,
) -> io::Result<()> {
    let start = Instant::now();
    while start.elapsed() < duration {
        r.state_mut::<Spinner>(id).tick();
        flush(r, stdout)?;
        thread::sleep(Duration::from_millis(80));
    }
    Ok(())
}
