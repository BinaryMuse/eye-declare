---
title: Reconciliation
description: How eye-declare matches components across rebuilds and preserves state
---

# Reconciliation

Reconciliation is the process by which eye-declare compares old and new component trees, decides which components to keep, add, or remove, and preserves state across rebuilds. Understanding reconciliation helps you write UIs that animate smoothly and don't lose state unexpectedly.

## How matching works

When you call a view function and return new `Elements`, the framework compares each new element against the existing tree:

1. **By key** — if an element has a `key`, the framework looks for an existing node with the same key *and* the same component type
2. **By position** — if no key, elements are matched by their index in the children list and their component type

When a match is found:
- The existing node's **state is preserved** (animations continue, input text survives, etc.)
- The component's **props are updated** to the new values
- `lifecycle()` runs again with the new props, potentially changing effects

When no match is found:
- Old unmatched nodes are **unmounted** (unmount effects fire, node tombstoned)
- New unmatched elements are **created** (fresh state, mount effects fire)

## Keys

Keys give components a stable identity that survives reordering:

```rust
element! {
    #(for msg in &state.messages {
        Markdown(key: msg.id.clone(), source: msg.text.clone())
    })
}
```

Without keys, if you remove the second item from a list of three, the framework matches by position:
- Position 0: old item A matches new item A (correct)
- Position 1: old item B matches new item C (wrong — B's state applied to C)
- Position 2: old item C has no match (unmounted)

With keys:
- Key "a": old A matches new A (correct)
- Key "b": old B has no match (unmounted correctly)
- Key "c": old C matches new C (correct, state preserved)

**Always use keys for dynamic lists.** Positional matching is fine for static content that doesn't change order.

### Key format

Keys are strings. Use whatever identifies the item uniquely:

```rust
// ID-based
key: msg.id.clone()

// Index-based (okay if items don't reorder)
key: format!("item-{i}")

// Composite
key: format!("{}-{}", section, item.id)
```

## Type matching

Reconciliation also checks component types. Even if keys match, a node is replaced if the component type changed:

```rust
element! {
    #(if state.loading {
        Spinner(key: "status", label: "Loading...")
    })
    #(if !state.loading {
        Text(key: "status") { "Done!" }
    })
}
```

Even though both use key `"status"`, switching from `Spinner` to `Text` creates a new node because the types differ. The Spinner's state is discarded and a fresh Text is created.

## State preservation

When a node is matched (same key + type, or same position + type), its state survives:

```rust
// On rebuild 1: Spinner mounts, starts animating
element! { Spinner(key: "s", label: "Step 1...") }

// On rebuild 2: Spinner matched by key, state preserved
// Animation continues seamlessly, label updates
element! { Spinner(key: "s", label: "Step 2...") }
```

This is why the Spinner doesn't restart its animation when the label changes — the framework recognizes it as the same component and preserves the internal animation frame counter.

## Reconciliation with containers

When a component has children, reconciliation happens recursively:

```rust
element! {
    VStack(key: "root") {
        Text(key: "header") { "..." }
        #(for item in &state.items {
            ItemComponent(key: item.id.clone(), ...)
        })
    }
}
```

1. The `VStack` is matched (same key + type)
2. Its children are reconciled against the previous children
3. Each `ItemComponent` is matched by key
4. New items create new nodes; removed items are unmounted

## The rebuild cycle

Here's the full sequence when state changes trigger a rebuild:

1. View function runs, producing new `Elements`
2. Framework reconciles new elements against existing tree
3. Matched nodes: props updated, state preserved
4. New nodes: created with initial state
5. Removed nodes: unmount effects fire, node tombstoned
6. `lifecycle()` runs for all live nodes
7. Context propagation happens
8. Heights measured (probe render for leaves, sum for containers)
9. `render()` output blitted for dirty nodes
10. Frame diffed and output emitted

## Tips

- Use keys for any dynamic list (loops with `#(for ...)`)
- Keys only need to be unique among siblings, not globally
- Positional matching works fine for static layouts
- If a component "resets" unexpectedly, check that it isn't losing its key match (changing type, missing key, or key collision)
- The framework matches by key + type, not key alone — this is intentional and prevents state confusion between different component types
