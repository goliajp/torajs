//! v0.3 #6 Graduation — torajs-core crate.
//!
//! The compiler library: lex → parse → desugar → typecheck → SSA
//! lower → inkwell-emit. Public modules let downstream callers
//! (`torajs-cli`, the conformance and bench harnesses) drive any
//! sub-stage of the pipeline directly.
//!
//! Depends on `torajs-runtime` for the C source files that get
//! embedded into every `tr build` artifact (refcount intrinsics,
//! string/array helpers, regex/Date engines).

pub mod ast;
pub mod check;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod ssa;
pub mod ssa_inkwell;
pub mod ssa_lower;
