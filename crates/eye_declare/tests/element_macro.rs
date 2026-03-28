use eye_declare::{
    BorderType, Column, Direction, Elements, HStack, InlineRenderer, Line, Markdown, Span, Spinner,
    TextBlock, VStack, View, WidthConstraint, element,
};

/// Helper: build elements into a renderer and return child count.
fn child_count(els: Elements) -> usize {
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    r.children(container).len()
}

#[test]
fn single_component_no_props() {
    let els = element! {
        VStack
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn single_component_with_props() {
    let els = element! {
        Spinner(label: "Loading...")
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn component_with_key() {
    let els = element! {
        Spinner(key: "s", label: "Loading...")
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    assert!(r.find_by_key(container, "s").is_some());
}

#[test]
fn component_with_children() {
    let els = element! {
        VStack {
            Spinner(label: "a")
            Spinner(label: "b")
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    // Container has one VStack child
    let vstack_id = r.children(container)[0];
    // VStack has two spinner children
    assert_eq!(r.children(vstack_id).len(), 2);
}

#[test]
fn string_literal_becomes_text_block() {
    let els = element! {
        VStack {
            "hello"
            "world"
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    assert_eq!(r.children(vstack_id).len(), 2);
}

#[test]
fn conditional_if_true() {
    let show = true;
    let els = element! {
        #(if show {
            Spinner(label: "visible")
        })
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn conditional_if_false() {
    let show = false;
    let els = element! {
        #(if show {
            Spinner(label: "hidden")
        })
    };
    assert_eq!(child_count(els), 0);
}

#[test]
fn conditional_if_else() {
    let loading = true;
    let els = element! {
        #(if loading {
            Spinner(label: "loading...")
        } else {
            TextBlock {
                Line {
                    Span(text: "done")
                }
            }
        })
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn conditional_if_let() {
    let tool: Option<String> = Some("cargo test".to_string());
    let els = element! {
        #(if let Some(ref t) = tool {
            Spinner(label: t.clone())
        })
    };
    assert_eq!(child_count(els), 1);

    let nothing: Option<String> = None;
    let els = element! {
        #(if let Some(ref _t) = nothing {
            Spinner(label: "nope")
        })
    };
    assert_eq!(child_count(els), 0);
}

#[test]
fn for_loop() {
    let items = ["alpha", "beta", "gamma"];
    let els = element! {
        #(for (i, item) in items.iter().enumerate() {
            Markdown(key: format!("item-{i}"), source: item.to_string())
        })
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    assert_eq!(r.children(container).len(), 3);
    assert!(r.find_by_key(container, "item-0").is_some());
    assert!(r.find_by_key(container, "item-1").is_some());
    assert!(r.find_by_key(container, "item-2").is_some());
}

#[test]
fn nested_components() {
    let els = element! {
        VStack {
            VStack {
                Spinner(label: "nested")
            }
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let outer = r.children(container)[0];
    let inner = r.children(outer)[0];
    assert_eq!(r.children(inner).len(), 1);
}

#[test]
fn mixed_content() {
    let messages = ["hello".to_string(), "world".to_string()];
    let thinking = true;

    let els = element! {
        VStack {
            #(for (i, msg) in messages.iter().enumerate() {
                Markdown(key: format!("msg-{i}"), source: msg.clone())
            })
            #(if thinking {
                Spinner(key: "thinking", label: "Thinking...")
            })
            "---"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    // 2 messages + 1 spinner + 1 text = 4
    assert_eq!(r.children(vstack_id).len(), 4);
}

#[test]
fn splice_elements_inline() {
    // Build Elements from a helper function
    fn sub_view() -> Elements {
        element! {
            Spinner(key: "s1", label: "one")
            Spinner(key: "s2", label: "two")
        }
    }

    let els = element! {
        VStack {
            "before"
            #(sub_view())
            "after"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    // "before" + 2 spliced spinners + "after" = 4
    assert_eq!(r.children(vstack_id).len(), 4);
    assert!(r.find_by_key(vstack_id, "s1").is_some());
    assert!(r.find_by_key(vstack_id, "s2").is_some());
}

#[test]
fn splice_variable() {
    let inner = element! {
        Markdown(key: "md", source: "hello".to_string())
    };

    let els = element! {
        VStack {
            #(inner)
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    assert_eq!(r.children(vstack_id).len(), 1);
    assert!(r.find_by_key(vstack_id, "md").is_some());
}

#[test]
fn splice_empty_elements() {
    let empty = Elements::new();

    let els = element! {
        VStack {
            "before"
            #(empty)
            "after"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    // "before" + empty splice + "after" = 2
    assert_eq!(r.children(vstack_id).len(), 2);
}

#[test]
fn splice_in_loop() {
    fn row(label: &str) -> Elements {
        element! {
            Spinner(label: label.to_string())
        }
    }

    let items = ["a", "b", "c"];
    let els = element! {
        VStack {
            #(for item in items {
                #(row(item))
            })
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    assert_eq!(r.children(vstack_id).len(), 3);
}

// ---------------------------------------------------------------------------
// Data children (TextBlock / Line / Span)
// ---------------------------------------------------------------------------

#[test]
fn text_block_with_line_span_children() {
    let els = element! {
        TextBlock {
            Line {
                Span(text: "hello")
            }
        }
    };
    // TextBlock absorbs data children — appears as a leaf in the element tree
    assert_eq!(child_count(els), 1);
}

#[test]
fn text_block_multiple_lines() {
    let els = element! {
        TextBlock {
            Line {
                Span(text: "line one")
            }
            Line {
                Span(text: "line two")
            }
        }
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn text_block_multi_span_line() {
    use ratatui_core::style::{Color, Style};

    let name = "World";
    let els = element! {
        TextBlock {
            Line {
                Span(text: "Hello, ", style: Style::default().fg(Color::Green))
                Span(text: name.to_string())
            }
        }
    };
    assert_eq!(child_count(els), 1);
}

#[test]
fn text_block_data_children_in_vstack() {
    let els = element! {
        VStack {
            TextBlock {
                Line {
                    Span(text: "first")
                }
            }
            TextBlock {
                Line {
                    Span(text: "second")
                }
            }
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    // Two TextBlock leaves
    assert_eq!(r.children(vstack_id).len(), 2);
}

#[test]
fn text_block_children_with_loop() {
    let items = ["alpha", "beta", "gamma"];
    let els = element! {
        VStack {
            #(for item in items {
                TextBlock {
                    Line {
                        Span(text: item.to_string())
                    }
                }
            })
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    assert_eq!(r.children(vstack_id).len(), 3);
}

#[test]
fn text_block_with_key() {
    let els = element! {
        VStack {
            TextBlock(key: "greeting") {
                Line {
                    Span(text: "hello")
                }
            }
        }
    };
    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    assert!(r.find_by_key(vstack_id, "greeting").is_some());
}

#[test]
fn text_block_children_render_content() {
    // Verify that data children actually flow through to rendered output,
    // not just that the tree structure is correct.
    let els = element! {
        TextBlock {
            Line {
                Span(text: "hello")
            }
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("hello"),
        "expected rendered output to contain 'hello', got: {:?}",
        output_str
    );
}

#[test]
fn text_block_multi_span_renders_both() {
    use ratatui_core::style::{Color, Style};

    let els = element! {
        TextBlock {
            Line {
                Span(text: "foo", style: Style::default().fg(Color::Red))
                Span(text: "bar")
            }
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("foo"),
        "expected 'foo' in output: {:?}",
        output_str
    );
    assert!(
        output_str.contains("bar"),
        "expected 'bar' in output: {:?}",
        output_str
    );
}

// ---------------------------------------------------------------------------
// HStack / Column layout
// ---------------------------------------------------------------------------

#[test]
fn hstack_with_columns() {
    let els = element! {
        HStack {
            Column(width: WidthConstraint::Fixed(6)) {
                "left"
            }
            Column {
                "right"
            }
        }
    };

    let mut r = InlineRenderer::new(20);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("left"),
        "expected 'left' in output: {:?}",
        output_str
    );
    assert!(
        output_str.contains("right"),
        "expected 'right' in output: {:?}",
        output_str
    );
}

#[test]
fn hstack_bare_children_default_to_fill() {
    // Components without Column wrapper should still work in HStack
    let els = element! {
        HStack {
            "one"
            "two"
        }
    };

    let mut r = InlineRenderer::new(20);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let hstack_id = r.children(container)[0];
    // Two children, both Fill (default)
    assert_eq!(r.children(hstack_id).len(), 2);
}

#[test]
fn hstack_mixed_columns_and_bare() {
    let els = element! {
        HStack {
            Column(width: WidthConstraint::Fixed(4)) {
                "fix"
            }
            "fill"
        }
    };

    let mut r = InlineRenderer::new(20);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("fix"),
        "expected 'fix' in output: {:?}",
        output_str
    );
    assert!(
        output_str.contains("fill"),
        "expected 'fill' in output: {:?}",
        output_str
    );
}

// ---------------------------------------------------------------------------
// View component
// ---------------------------------------------------------------------------

#[test]
fn view_default_with_children() {
    let els = element! {
        View {
            "hello"
            "world"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let view_id = r.children(container)[0];
    assert_eq!(r.children(view_id).len(), 2);
}

#[test]
fn view_row_direction() {
    let els = element! {
        View(direction: Direction::Row) {
            View(width: WidthConstraint::Fixed(10)) {
                "left"
            }
            View {
                "right"
            }
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("left"),
        "expected 'left' in output: {:?}",
        output_str
    );
    assert!(
        output_str.contains("right"),
        "expected 'right' in output: {:?}",
        output_str
    );
}

#[test]
fn view_with_border_renders_content() {
    let els = element! {
        View(border: BorderType::Plain) {
            "inside"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    // Border corners should be present
    assert!(
        output_str.contains("┌"),
        "expected top-left border in output: {:?}",
        output_str
    );
    assert!(
        output_str.contains("inside"),
        "expected 'inside' in output: {:?}",
        output_str
    );
}

#[test]
fn view_with_props_and_key() {
    let els = element! {
        View(key: "card", border: BorderType::Rounded, padding: 1) {
            "content"
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    assert!(r.find_by_key(container, "card").is_some());
}

#[test]
fn view_nested() {
    let els = element! {
        View(border: BorderType::Plain) {
            View(border: BorderType::Plain) {
                "nested"
            }
        }
    };

    let mut r = InlineRenderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);

    let output = r.render();
    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("nested"),
        "expected 'nested' in output: {:?}",
        output_str
    );
}
