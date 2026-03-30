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

// ---------------------------------------------------------------------------
// #[props] attribute macro tests
// ---------------------------------------------------------------------------

mod props_tests {
    use eye_declare::{
        Canvas, Component, Elements, InlineRenderer, VStack, View, element, impl_slot_children,
        props,
    };
    use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
    use ratatui_widgets::{borders::BorderType, paragraph::Paragraph};

    #[props]
    struct BadgeProps {
        pub label: String,
        #[default(true)]
        pub visible: bool,
    }

    impl Component for BadgeProps {
        type State = ();

        fn view(&self, _state: &(), _children: Elements) -> Elements {
            if !self.visible {
                return Elements::new();
            }
            let label = self.label.clone();
            let mut els = Elements::new();
            els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
                Paragraph::new(label.as_str()).render(area, buf);
            }));
            els
        }
    }

    #[test]
    fn props_default_values_work_in_element_macro() {
        // visible defaults to true via #[default(true)]
        let els = element! {
            BadgeProps(label: "hello".to_string())
        };

        let mut r = InlineRenderer::new(20);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("hello"),
            "badge should render with default visible=true"
        );
    }

    #[test]
    fn props_override_default() {
        // Explicitly set visible to false
        let els = element! {
            BadgeProps(label: "hidden".to_string(), visible: false)
        };

        let mut r = InlineRenderer::new(20);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            !output_str.contains("hidden"),
            "badge should not render when visible=false"
        );
    }

    #[props]
    struct CardProps {
        pub title: String,
    }

    impl Component for CardProps {
        type State = ();

        fn view(&self, _state: &(), children: Elements) -> Elements {
            let mut els = Elements::new();
            els.add_with_children(
                View {
                    border: Some(BorderType::Rounded),
                    title: Some(self.title.clone()),
                    ..View::default()
                },
                children,
            );
            els
        }
    }

    impl_slot_children!(CardProps);

    #[test]
    fn props_with_slot_children() {
        let els = element! {
            CardProps(title: "Test Card".to_string()) {
                "inside"
            }
        };

        let mut r = InlineRenderer::new(30);
        let container = r.push(VStack);
        r.rebuild(container, els);

        // CardProps wraps children in a bordered View — verify the tree
        // has the expected structure (CardProps > View > TextBlock)
        let card_id = r.children(container)[0];
        let view_id = r.children(card_id)[0];
        let text_id = r.children(view_id)[0];
        // Verify the tree depth — child content is inside the border
        assert_eq!(r.children(container).len(), 1, "one card");
        assert_eq!(r.children(card_id).len(), 1, "card has one view child");
        assert_eq!(r.children(view_id).len(), 1, "view has one text child");
        assert!(r.children(text_id).is_empty(), "text is a leaf");
    }
}

// ---------------------------------------------------------------------------
// #[component] attribute macro tests
// ---------------------------------------------------------------------------

mod component_tests {
    use eye_declare::{
        Canvas, Elements, Hooks, InlineRenderer, VStack, View, component, element, props,
    };
    use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
    use ratatui_widgets::{borders::BorderType, paragraph::Paragraph};
    use std::time::Duration;

    // --- Stateless component with children ---

    #[props]
    struct CardProps {
        title: String,
        #[default(true)]
        visible: bool,
    }

    #[component(props = CardProps, children = Elements)]
    fn card(props: &CardProps, children: Elements) -> Elements {
        if !props.visible {
            return Elements::new();
        }
        let mut els = Elements::new();
        els.add_with_children(
            View {
                border: Some(BorderType::Rounded),
                title: Some(props.title.clone()),
                ..View::default()
            },
            children,
        );
        els
    }

    #[test]
    fn stateless_component_with_children() {
        let els = element! {
            CardProps(title: "Test") {
                "inside"
            }
        };

        let mut r = InlineRenderer::new(30);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let card_id = r.children(container)[0];
        assert_eq!(r.children(card_id).len(), 1, "card has view child");
    }

    #[test]
    fn required_prop_enforced() {
        // title is required (no #[default]) — this compiles because we provide it
        let els = element! {
            CardProps(title: "Required") {
                "body"
            }
        };
        let mut r = InlineRenderer::new(30);
        let container = r.push(VStack);
        r.rebuild(container, els);
        assert_eq!(r.children(container).len(), 1);
    }

    #[test]
    fn optional_prop_uses_default() {
        // visible defaults to true, so card renders
        let els = element! {
            CardProps(title: "Visible") {
                "body"
            }
        };
        let mut r = InlineRenderer::new(30);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let card_id = r.children(container)[0];
        // Card has children (visible=true by default)
        assert!(!r.children(card_id).is_empty());
    }

    // --- Stateless leaf component ---

    #[props]
    struct BadgeProps {
        label: String,
    }

    #[component(props = BadgeProps)]
    fn badge(props: &BadgeProps) -> Elements {
        let label = props.label.clone();
        let mut els = Elements::new();
        els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
            Paragraph::new(label.as_str()).render(area, buf);
        }));
        els
    }

    #[test]
    fn stateless_leaf_component() {
        let els = element! {
            BadgeProps(label: "hello")
        };

        let mut r = InlineRenderer::new(20);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("hello"));
    }

    // --- Stateful component with hooks ---

    #[derive(Default)]
    struct CounterState {
        count: u32,
    }

    #[props]
    struct CounterProps {
        #[default("Count".to_string())]
        label: String,
    }

    #[component(props = CounterProps, state = CounterState)]
    fn counter(
        props: &CounterProps,
        state: &CounterState,
        hooks: &mut Hooks<CounterState>,
    ) -> Elements {
        hooks.use_interval(Duration::from_millis(100), |s| s.count += 1);

        let text = format!("{}: {}", props.label, state.count);
        let mut els = Elements::new();
        els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
            Paragraph::new(text.as_str()).render(area, buf);
        }));
        els
    }

    #[test]
    fn stateful_component_with_hooks() {
        let els = element! {
            CounterProps(label: "Items")
        };

        let mut r = InlineRenderer::new(30);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("Items:") && output_str.contains("0"),
            "initial render should contain label and count: {}",
            output_str
        );
    }

    // --- Data children component ---

    /// A styled label — data child of StyledList.
    #[derive(Clone)]
    struct StyledLabel {
        text: String,
    }

    impl StyledLabel {
        fn builder() -> StyledLabelBuilder {
            StyledLabelBuilder {
                text: String::new(),
            }
        }
    }

    struct StyledLabelBuilder {
        text: String,
    }

    impl StyledLabelBuilder {
        fn text(mut self, t: impl Into<String>) -> Self {
            self.text = t.into();
            self
        }
        fn build(self) -> StyledLabel {
            StyledLabel { text: self.text }
        }
    }

    /// Child enum for StyledList.
    enum ListChild {
        Label(StyledLabel),
    }

    impl From<StyledLabel> for ListChild {
        fn from(l: StyledLabel) -> Self {
            ListChild::Label(l)
        }
    }

    #[props]
    struct StyledListProps {
        #[default("List".to_string())]
        title: String,
    }

    #[component(props = StyledListProps, children = eye_declare::DataChildren<ListChild>)]
    fn styled_list(
        props: &StyledListProps,
        children: &eye_declare::DataChildren<ListChild>,
    ) -> Elements {
        let mut labels: Vec<String> = Vec::new();
        for child in children.as_slice() {
            match child {
                ListChild::Label(l) => labels.push(l.text.clone()),
            }
        }
        let text = format!("{}: {}", props.title, labels.join(", "));
        let mut els = Elements::new();
        els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
            Paragraph::new(text.as_str()).render(area, buf);
        }));
        els
    }

    #[test]
    fn data_children_component_with_children() {
        let els = element! {
            StyledListProps(title: "Colors") {
                StyledLabel(text: "red")
                StyledLabel(text: "blue")
            }
        };

        let mut r = InlineRenderer::new(40);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        // ANSI diff skips space characters (cursor movements), so check
        // for content fragments rather than the exact combined string.
        assert!(
            output_str.contains("Colors:")
                && output_str.contains("red")
                && output_str.contains("blue"),
            "should render title + data children: {}",
            output_str
        );
    }

    #[test]
    fn data_children_component_without_children() {
        // Used without braces — gets default (empty) data children
        let els = element! {
            StyledListProps(title: "Empty")
        };

        let mut r = InlineRenderer::new(40);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("Empty:"),
            "should render title with no children: {}",
            output_str
        );
    }

    // --- Data children with state + initial_state ---

    #[derive(Default)]
    pub struct AccumState {
        pub render_count: u32,
    }

    /// Child type for the stateful data-children component.
    pub enum AccumChild {
        Label(String),
    }

    impl From<String> for AccumChild {
        fn from(s: String) -> Self {
            AccumChild::Label(s)
        }
    }

    #[props]
    struct AccumProps {
        #[default("header".to_string())]
        header: String,
    }

    #[component(
        props = AccumProps,
        state = AccumState,
        initial_state = AccumState { render_count: 1 },
        children = eye_declare::DataChildren<AccumChild>
    )]
    fn accum(
        props: &AccumProps,
        state: &AccumState,
        children: &eye_declare::DataChildren<AccumChild>,
    ) -> Elements {
        let labels: Vec<&str> = children
            .as_slice()
            .iter()
            .map(|c| match c {
                AccumChild::Label(s) => s.as_str(),
            })
            .collect();
        let text = format!(
            "{} (n={}) [{}]",
            props.header,
            state.render_count,
            labels.join(", ")
        );
        let mut els = Elements::new();
        els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
            Paragraph::new(text.as_str()).render(area, buf);
        }));
        els
    }

    #[test]
    fn data_children_with_state_and_initial_state() {
        // Verifies that initial_state works correctly for both the
        // props-type Component impl and the wrapper Component impl.
        let els = element! {
            AccumProps(header: "test") {
                // String children go through From<String> for AccumChild
            }
        };

        let mut r = InlineRenderer::new(40);
        let container = r.push(VStack);
        r.rebuild(container, els);

        let output = r.render();
        let output_str = String::from_utf8_lossy(&output);
        // initial_state sets render_count = 1; wrapper delegates to props
        assert!(
            output_str.contains("n=1"),
            "wrapper initial_state should delegate to props: {}",
            output_str
        );
    }
}
