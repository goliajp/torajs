//! The analysis is a pure function of a function body and the module's
//! function names. Later passes that want the same facts for a body no
//! pass has changed since read them here instead of recomputing; any
//! difference in the body or the names is a miss.

use crate::id_hash::HashMap;
use std::borrow::Cow;

use torajs_core::ssa::{Function, Module, ValueId};

use super::NumFact;

/// The facts a point evaluator can start from instead of analyzing.
pub type Seed<'a> = Option<&'a HashMap<ValueId, NumFact>>;

pub struct FactCache {
    names: Vec<String>,
    /// index-aligned with `Module::funcs`: the body analyzed and the
    /// facts it gave
    entries: Vec<(Function, HashMap<ValueId, NumFact>)>,
}

impl FactCache {
    /// `facts` is what [`super::analyze_module`] gave for `module`.
    pub fn new(module: &Module, facts: Vec<HashMap<ValueId, NumFact>>) -> Self {
        FactCache {
            names: module.funcs.iter().map(|f| f.name.clone()).collect(),
            entries: module.funcs.iter().cloned().zip(facts).collect(),
        }
    }

    /// `module`'s function names, and `cache` when it was built under
    /// those names (a pass that adds, drops or renames a function
    /// invalidates every entry). A matching cache lends its own copy
    /// of the names.
    pub fn for_module<'a>(
        cache: Option<&'a FactCache>,
        module: &Module,
    ) -> (Option<&'a FactCache>, Cow<'a, [String]>) {
        match cache {
            Some(c) if c.names.iter().eq(module.funcs.iter().map(|f| &f.name)) => {
                (Some(c), Cow::Borrowed(c.names.as_slice()))
            }
            _ => (
                None,
                Cow::Owned(module.funcs.iter().map(|f| f.name.clone()).collect()),
            ),
        }
    }

    /// The facts for `module.funcs[idx]` when that body is the one
    /// analyzed.
    pub fn get(&self, idx: usize, func: &Function) -> Option<&HashMap<ValueId, NumFact>> {
        let (body, facts) = self.entries.get(idx)?;
        (body == func).then_some(facts)
    }
}
