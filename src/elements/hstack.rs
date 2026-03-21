use crate::component::HStack;
use crate::element::Element;
use crate::node::{Layout, NodeId};
use crate::renderer::Renderer;

/// Element builder for an [`HStack`] container.
///
/// Children are laid out left-to-right. Each child's width is
/// determined by its [`WidthConstraint`](crate::node::WidthConstraint)
/// (set via [`ElementHandle::width`](crate::element::ElementHandle::width)).
///
/// ```ignore
/// let mut row = Elements::new();
/// row.add(TextBlockEl::new().unstyled(">")).width(WidthConstraint::Fixed(2));
/// row.add(MarkdownEl::new(content)); // default: Fill
/// els.add_with_children(HStackEl, row);
///
/// // Or use the shorthand:
/// els.hstack(row);
/// ```
pub struct HStackEl;

impl Element for HStackEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, HStack);
        renderer.set_layout(id, Layout::Horizontal);
        id
    }

    fn update(self: Box<Self>, _renderer: &mut Renderer, _node_id: NodeId) {
        // HStack has no props to update. Layout direction is preserved.
    }
}
