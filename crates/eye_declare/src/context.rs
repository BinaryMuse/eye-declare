use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Type-keyed map for component context values.
///
/// Used internally by the framework to propagate values down the
/// component tree during reconciliation. Components provide context
/// via [`Hooks::provide_context`](crate::Hooks::provide_context)
/// and consume it via [`Hooks::use_context`](crate::Hooks::use_context).
pub(crate) struct ContextMap {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl ContextMap {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Look up a context value by TypeId, returning it as `&(dyn Any + Send + Sync)`.
    pub fn get_by_type_id(&self, type_id: TypeId) -> Option<&(dyn Any + Send + Sync)> {
        self.values.get(&type_id).map(|v| &**v)
    }

    /// Insert a context value, returning the previous value if any.
    pub fn insert(
        &mut self,
        type_id: TypeId,
        value: Box<dyn Any + Send + Sync>,
    ) -> Option<Box<dyn Any + Send + Sync>> {
        self.values.insert(type_id, value)
    }

    /// Remove a context value by TypeId.
    pub fn remove(&mut self, type_id: &TypeId) -> Option<Box<dyn Any + Send + Sync>> {
        self.values.remove(type_id)
    }
}

/// Saved context entries for push/pop during tree traversal.
///
/// Each entry is a `(TypeId, Option<old_value>)` pair. On pop, the
/// old value is restored (or the key is removed if `None`).
pub(crate) type SavedContext = Vec<(TypeId, Option<Box<dyn Any + Send + Sync>>)>;

/// Context values provided by a component for its descendants.
pub(crate) type ProvidedContexts = Vec<(TypeId, Box<dyn Any + Send + Sync>)>;
