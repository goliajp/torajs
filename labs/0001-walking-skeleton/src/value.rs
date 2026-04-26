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
    /// Reference to a host function by index into `IrModule.host_fns`.
    HostFn(u32),
}
