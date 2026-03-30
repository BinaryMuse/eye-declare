use crate::component::Component;
use crate::element::{ElementHandle, Elements};
use crate::node::WidthConstraint;

// ---------------------------------------------------------------------------
// AddTo — how a value adds itself to a collector
// ---------------------------------------------------------------------------

/// Trait for adding a value to a child collector.
///
/// The `element!` macro dispatches all child additions through this trait,
/// enabling **compile-time type checking** of parent-child relationships.
/// If you try to nest a component inside a parent that doesn't accept it,
/// you'll get a compile error rather than a runtime panic.
///
/// # Implementations
///
/// - **Blanket impl**: any [`Component`] can be added to [`Elements`].
/// - **Data types**: [`Span`](crate::Span) converts `Into<TextChild>`,
///   `String` converts `Into<TextChild>`. These produce compile errors
///   if used in the wrong context.
pub trait AddTo<Collector: ?Sized> {
    /// Handle returned after adding. Supports `.key()` / `.width()` chaining.
    type Handle<'a>
    where
        Collector: 'a;

    /// Add this value to the collector, returning a handle for chaining
    /// `.key()` and `.width()`.
    fn add_to(self, collector: &mut Collector) -> Self::Handle<'_>;
}

/// Blanket: any Component can be added to Elements.
impl<C: Component> AddTo<Elements> for C {
    type Handle<'a> = ElementHandle<'a>;

    fn add_to(self, els: &mut Elements) -> ElementHandle<'_> {
        els.add(self)
    }
}

/// String → Elements: creates a Text component with the string as content.
///
/// This powers the `element!` string literal sugar — `"hello"` becomes a
/// [`Text`](crate::Text) component. The same `AddTo` dispatch also works
/// inside data children contexts (e.g., `Text { "hello" }`) via the
/// `Into<TextChild>` blanket impl.
impl AddTo<Elements> for String {
    type Handle<'a> = ElementHandle<'a>;

    fn add_to(self, els: &mut Elements) -> ElementHandle<'_> {
        let text = crate::components::text::Text::unstyled(self);
        els.add(text)
    }
}

// ---------------------------------------------------------------------------
// SpliceInto — how Elements splice into a collector
// ---------------------------------------------------------------------------

/// Trait for splicing an [`Elements`] list inline into a collector.
///
/// Used by the `element!` macro's `#(expr)` syntax. Only implemented
/// for `Elements` → `Elements` — using `#(expr)` inside a data
/// collector (e.g., inside a `Line { }` block) produces a compile error.
pub trait SpliceInto<Collector: ?Sized> {
    /// Splice all entries from this value into the collector.
    fn splice_into(self, collector: &mut Collector);
}

impl SpliceInto<Elements> for Elements {
    fn splice_into(self, collector: &mut Elements) {
        collector.splice(self);
    }
}

// ---------------------------------------------------------------------------
// ChildCollector — how a component collects and finalizes children
// ---------------------------------------------------------------------------

/// Declares how a component collects children in the `element!` macro.
///
/// Implement this trait to allow your component to accept children in
/// `element!` braces. The `Collector` type determines which child types
/// are valid (via [`AddTo`]).
///
/// # Two patterns
///
/// - **Slot children** (layout containers like [`VStack`](crate::VStack)):
///   Use `Elements` as `Collector` and [`ComponentWithSlot`] as `Output`.
///   The [`impl_slot_children!`](crate::impl_slot_children) macro does this
///   automatically.
///
/// - **Data children** (like [`Text`](crate::Text)):
///   Use a custom collector type. For `#[component]` functions, the macro
///   generates a wrapper that holds the collected data.
///
/// Components without `ChildCollector` produce a compile error when used
/// with children in `element!`.
pub trait ChildCollector: Sized {
    /// The type used to accumulate children.
    type Collector: Default;

    /// The output type after finalizing children.
    ///
    /// For layout containers: [`ComponentWithSlot<Self>`].
    /// For data components: `Self` (with data absorbed).
    type Output;

    /// Finalize children collection, producing the output value.
    fn finish(self, collector: Self::Collector) -> Self::Output;
}

// ---------------------------------------------------------------------------
// ComponentWithSlot — wrapper for component + slot children
// ---------------------------------------------------------------------------

/// Wrapper pairing a component with its slot children.
///
/// Produced by [`ChildCollector::finish`] for layout containers.
/// The `element!` macro creates this automatically when you write
/// `Component { children... }` for a component that uses
/// [`impl_slot_children!`](crate::impl_slot_children).
pub struct ComponentWithSlot<C> {
    component: C,
    children: Elements,
}

impl<C> ComponentWithSlot<C> {
    /// Create a new component-with-slot wrapper.
    pub fn new(component: C, children: Elements) -> Self {
        Self {
            component,
            children,
        }
    }
}

impl<C: Component> AddTo<Elements> for ComponentWithSlot<C> {
    type Handle<'a> = ElementHandle<'a>;

    fn add_to(self, els: &mut Elements) -> ElementHandle<'_> {
        els.add_with_children(self.component, self.children)
    }
}

// ---------------------------------------------------------------------------
// DataChildren<T> — generic collector for typed data children via Into<T>
// ---------------------------------------------------------------------------

/// Generic collector for components that accept typed data children.
///
/// `DataChildren<T>` collects children via `Into<T>` conversions, where `T`
/// is a child enum defined by the component. This replaces per-component
/// collector types with a single generic pattern.
///
/// # Example
///
/// ```ignore
/// // Component defines what children it accepts
/// enum TextChild {
///     Span(Span),
/// }
///
/// impl From<Span> for TextChild {
///     fn from(s: Span) -> Self { TextChild::Span(s) }
/// }
///
/// // #[component] generates the ChildCollector automatically:
/// // #[component(props = MyText, children = DataChildren<TextChild>)]
/// // fn my_text(props: &MyText, children: &DataChildren<TextChild>) -> Elements { ... }
/// ```
pub struct DataChildren<T>(Vec<T>);

impl<T> DataChildren<T> {
    /// Consume the collector and return the collected children.
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Borrow the collected children as a slice.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> Default for DataChildren<T> {
    fn default() -> Self {
        DataChildren(Vec::new())
    }
}

/// Any type that converts `Into<T>` can be added to a `DataChildren<T>` collector.
impl<T, V: Into<T>> AddTo<DataChildren<T>> for V {
    type Handle<'a>
        = DataHandle
    where
        T: 'a;

    fn add_to(self, collector: &mut DataChildren<T>) -> DataHandle {
        collector.0.push(self.into());
        DataHandle
    }
}

// ---------------------------------------------------------------------------
// DataHandle — no-op handle for data child additions
// ---------------------------------------------------------------------------

/// No-op handle returned when adding data children (e.g., [`Span`](crate::Span))
/// to a custom collector.
///
/// Provides `.key()` and `.width()` methods that silently do nothing,
/// so the `element!` macro's chaining syntax compiles in all contexts.
/// Keys and width constraints are only meaningful on [`Elements`] entries.
pub struct DataHandle;

impl DataHandle {
    /// No-op — keys are only meaningful on [`Elements`] entries.
    pub fn key(self, _key: impl Into<String>) -> Self {
        self
    }

    /// No-op — width constraints are only meaningful on [`Elements`] entries.
    pub fn width(self, _constraint: WidthConstraint) -> Self {
        self
    }
}
