use std::any::TypeId;

use crate::component::Component;
use crate::node::{NodeId, WidthConstraint};
use crate::renderer::Renderer;

/// Type-erased component description for the element tree.
///
/// Users don't implement this directly — implement [`Component`]
/// instead, which gets `Element` automatically via blanket impl.
pub(crate) trait Element: Send {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId;
    fn update(self: Box<Self>, _renderer: &mut Renderer, _node_id: NodeId) {}
}

/// Blanket implementation: every Component is automatically an Element.
///
/// - **Build**: creates a new node via `append_child` (calls `initial_state()`)
/// - **Update**: swaps the component on the existing node (preserves state)
impl<C: Component> Element for C {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        renderer.append_child_inner(parent, *self)
    }

    fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
        renderer.swap_component(node_id, *self);
    }
}

/// An entry in an Elements list: a component with optional children.
pub(crate) struct ElementEntry {
    pub(crate) element: Box<dyn Element>,
    pub(crate) children: Option<Elements>,
    pub(crate) key: Option<String>,
    pub(crate) type_id: TypeId,
    pub(crate) width_constraint: WidthConstraint,
}

/// A list of component descriptions for declarative tree building.
///
/// `Elements` is what view functions return and what the framework
/// reconciles against the existing component tree. Build one with the
/// [`element!`](crate::element!) macro or the imperative API:
///
/// ```ignore
/// // With the element! macro (preferred):
/// fn my_view(state: &MyState) -> Elements {
///     element! {
///         "Hello"
///         #(if state.loading {
///             Spinner(key: "spinner", label: "Loading...")
///         })
///     }
/// }
///
/// // Imperative API:
/// fn my_view(state: &MyState) -> Elements {
///     let mut els = Elements::new();
///     els.add(TextBlock::new().unstyled("Hello"));
///     if state.loading {
///         els.add(Spinner::new("Loading...")).key("spinner");
///     }
///     els
/// }
/// ```
pub struct Elements {
    items: Vec<ElementEntry>,
}

impl Elements {
    /// Create an empty element list.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add a component to the list.
    ///
    /// Returns an [`ElementHandle`] that can be used to set a key
    /// for stable identity across rebuilds.
    pub fn add<C: Component>(&mut self, component: C) -> ElementHandle<'_> {
        let type_id = TypeId::of::<C>();
        self.items.push(ElementEntry {
            element: Box::new(component),
            children: None,
            key: None,
            type_id,
            width_constraint: WidthConstraint::default(),
        });
        ElementHandle {
            entry: self.items.last_mut().unwrap(),
        }
    }

    /// Add a raw Element implementation (internal use only).
    ///
    /// Used by tests that need custom build/update behavior not
    /// expressible through the Component trait alone.
    #[allow(dead_code)]
    pub(crate) fn add_element<E: Element + 'static>(&mut self, element: E) -> ElementHandle<'_> {
        let type_id = TypeId::of::<E>();
        self.items.push(ElementEntry {
            element: Box::new(element),
            children: None,
            key: None,
            type_id,
            width_constraint: WidthConstraint::default(),
        });
        ElementHandle {
            entry: self.items.last_mut().unwrap(),
        }
    }

    /// Add a raw Element with nested children (internal use only).
    #[allow(dead_code)]
    pub(crate) fn add_element_with_children<E: Element + 'static>(
        &mut self,
        element: E,
        children: Elements,
    ) -> ElementHandle<'_> {
        let type_id = TypeId::of::<E>();
        self.items.push(ElementEntry {
            element: Box::new(element),
            children: Some(children),
            key: None,
            type_id,
            width_constraint: WidthConstraint::default(),
        });
        ElementHandle {
            entry: self.items.last_mut().unwrap(),
        }
    }

    /// Add a component with nested children.
    ///
    /// The component is created first, then children are built as its
    /// descendants. The component's `children()` method receives
    /// these as the `slot` parameter.
    pub fn add_with_children<C: Component>(
        &mut self,
        component: C,
        children: Elements,
    ) -> ElementHandle<'_> {
        let type_id = TypeId::of::<C>();
        self.items.push(ElementEntry {
            element: Box::new(component),
            children: Some(children),
            key: None,
            type_id,
            width_constraint: WidthConstraint::default(),
        });
        ElementHandle {
            entry: self.items.last_mut().unwrap(),
        }
    }

    /// Add a [`VStack`](crate::VStack) wrapper around the given children.
    ///
    /// Equivalent to `add_with_children(VStack, children)`.
    pub fn group(&mut self, children: Elements) -> ElementHandle<'_> {
        self.add_with_children(crate::component::VStack, children)
    }

    /// Add an [`HStack`](crate::HStack) wrapper around the given children.
    ///
    /// Children default to [`WidthConstraint::Fill`].
    /// Use `.width(WidthConstraint::Fixed(n))` for fixed-width children.
    pub fn hstack(&mut self, children: Elements) -> ElementHandle<'_> {
        self.add_with_children(crate::component::HStack, children)
    }

    /// Splice another Elements list into this one.
    ///
    /// All entries from `other` are appended to this list. This is used
    /// by the `element!` macro's `#(expr)` syntax to interpolate
    /// pre-built Elements inline.
    pub fn splice(&mut self, other: Elements) {
        self.items.extend(other.items);
    }

    /// Consume the Elements and return the entries for reconciliation.
    pub(crate) fn into_items(self) -> Vec<ElementEntry> {
        self.items
    }
}

impl Default for Elements {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle returned by [`Elements::add`] for chaining `.key()` and `.width()`.
///
/// This handle has a builder-style API — call methods on it immediately
/// after adding a component to an [`Elements`] list:
///
/// ```ignore
/// let mut els = Elements::new();
/// els.add(Spinner::new("loading..."))
///     .key("my-spinner")
///     .width(WidthConstraint::Fixed(20));
/// ```
pub struct ElementHandle<'a> {
    entry: &'a mut ElementEntry,
}

impl<'a> ElementHandle<'a> {
    /// Set a key for stable identity across rebuilds.
    ///
    /// Keyed elements are matched by key during reconciliation,
    /// allowing them to survive position changes. Without a key,
    /// elements are matched by position and type.
    pub fn key(self, key: impl Into<String>) -> Self {
        self.entry.key = Some(key.into());
        self
    }

    /// Set the width constraint for this element within a horizontal container.
    ///
    /// Only meaningful when the element is a child of an HStack.
    /// `Fixed(n)` reserves exactly n columns. `Fill` (default) takes
    /// remaining space, split equally among Fill siblings.
    pub fn width(self, constraint: WidthConstraint) -> Self {
        self.entry.width_constraint = constraint;
        self
    }
}
