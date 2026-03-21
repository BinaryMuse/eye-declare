use std::any::TypeId;

use crate::node::{NodeId, WidthConstraint};
use crate::renderer::Renderer;

/// Describes a component to create in the tree.
///
/// Each built-in component has a corresponding element builder (e.g.,
/// `TextBlockEl`, `SpinnerEl`). Users can implement this trait for
/// custom components.
///
/// The `build` method creates the component, adds it as a child of
/// `parent`, initializes its state, and returns the new NodeId.
///
/// The `update` method is called during reconciliation when a matching
/// node is found. It should update "props" (parent-provided config)
/// while preserving "local state" (component-internal state like
/// animation frames). Implement `update` for any element whose
/// configuration can change across rebuilds.
pub trait Element: Send {
    /// Create the component, add it as a child of `parent`,
    /// and initialize its state. Returns the new NodeId.
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId;

    /// Update an existing node with new configuration from this element.
    ///
    /// Called during reconciliation when a matching node is found.
    /// Override this to update props while preserving local state.
    /// Default: no-op (keeps existing state unchanged).
    fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
        let _ = (renderer, node_id);
    }
}

/// An entry in an Elements list: an element description with optional children.
pub(crate) struct ElementEntry {
    pub(crate) element: Box<dyn Element>,
    pub(crate) children: Option<Elements>,
    pub(crate) key: Option<String>,
    pub(crate) type_id: TypeId,
    pub(crate) width_constraint: WidthConstraint,
}

/// A list of element descriptions for declarative tree building.
///
/// Used with `Renderer::rebuild` to describe what the tree should
/// look like. View functions return `Elements`.
///
/// ```ignore
/// fn my_view(state: &MyState) -> Elements {
///     let mut els = Elements::new();
///     els.add(TextBlockEl::new().unstyled("Hello"));
///     if state.loading {
///         els.add(SpinnerEl::new("Loading...")).key("spinner");
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

    /// Add an element to the list.
    ///
    /// Returns an [`ElementHandle`] that can be used to set a key
    /// for stable identity across rebuilds.
    pub fn add<E: Element + 'static>(&mut self, element: E) -> ElementHandle<'_> {
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

    /// Add an element with nested children.
    ///
    /// The element is created first, then children are built as its
    /// descendants.
    pub fn add_with_children<E: Element + 'static>(
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

    /// Add a VStack wrapper around the given children.
    ///
    /// Shorthand for `add_with_children(VStackEl, children)`.
    pub fn group(&mut self, children: Elements) -> ElementHandle<'_> {
        self.add_with_children(crate::elements::VStackEl, children)
    }

    /// Add an HStack wrapper around the given children.
    ///
    /// Shorthand for `add_with_children(HStackEl, children)`.
    /// Children declare their width with `.width(WidthConstraint::Fixed(n))`.
    pub fn hstack(&mut self, children: Elements) -> ElementHandle<'_> {
        self.add_with_children(crate::elements::HStackEl, children)
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

/// Handle returned by [`Elements::add`] for setting element keys.
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
