//! Runtime value. v0 layout — interpreter only.
//!
//! NB: this module is the future entry point for swapping in NaN-boxing.
//! All callers MUST go through methods, never field access on variants.

use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Undefined,
    Number(f64),
    Bool(bool),
    String(Rc<String>),
    /// v0 array — shared, immutable. Mutation via `a[i] = x` lands at P2 with
    /// proper ownership rules.
    Array(Rc<Vec<Value>>),
    /// Reference to a host function by index into `IrModule.host_fns`.
    HostFn(u32),
    /// Reference to a user function by index into `IrModule.functions`.
    Function(u32),
}
