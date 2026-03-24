use eye_declare::{Elements, Markdown, Renderer, Spinner, TextBlock, VStack, element};
use ratatui_core::style::Style;

/// Helper: build elements into a renderer and return child count.
fn child_count(els: Elements) -> usize {
    let mut r = Renderer::new(40);
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
    let mut r = Renderer::new(40);
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
    let mut r = Renderer::new(40);
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
    let mut r = Renderer::new(40);
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
            TextBlock(lines: vec![("done".to_string(), Style::default())])
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
    let mut r = Renderer::new(40);
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
    let mut r = Renderer::new(40);
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

    let mut r = Renderer::new(40);
    let container = r.push(VStack);
    r.rebuild(container, els);
    let vstack_id = r.children(container)[0];
    // 2 messages + 1 spinner + 1 text = 4
    assert_eq!(r.children(vstack_id).len(), 4);
}
