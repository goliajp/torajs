//! One-pass matcher construction (RE2 `onepass` / regex-automata
//! `OnePass` shape) over a compiled [`crate::program::Program`].
//!
//! A program is one-pass when, at every position, at most one thread
//! of the leftmost-first Pike VM can consume the next code unit. Such
//! a program needs no thread list and no second pass for captures:
//! the walk is a DFA whose edges carry the capture-slot writes the
//! Pike VM would have made on its ε path, and whose accepts carry the
//! writes on the path to `Op::Match`.
//!
//! - [`analyze`] decides one-passness and produces the [`ir::MatcherIr`]
//!   for one payload encoding: Latin-1 code units or UTF-16 ones.
//! - [`ir`] is the matcher's shape plus a reference interpreter over
//!   the code units; the AOT compiler emits the same walk as a
//!   native function.

pub mod analyze;
pub mod ir;

pub use analyze::build;
pub use ir::{Edge, MatcherIr, State};
