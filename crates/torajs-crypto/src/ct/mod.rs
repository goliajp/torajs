//! Constant-time comparison, selection and wiping.
//!
//! Every comparison or selection that involves a secret goes through this
//! module. Lengths are treated as public: [`ct_eq`] returns early when the
//! two slices differ in length, never on their contents.

mod choice;
pub mod zeroize;

pub use choice::{Choice, Word, cmov, ct_eq, ct_select};
