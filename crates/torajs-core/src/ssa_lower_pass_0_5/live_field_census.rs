//! Whole-program census: which class fields hold a live cell at every
//! read.
//!
//! The factory seeds a pointer-shaped field with the immortal
//! `undefined` cell and the constructor overwrites it, so a read of
//! such a field tests for the cell: in general the write may not have
//! run yet (a read inside the constructor ahead of it), or a later
//! write may have put the cell back (a value typed `any`, an element
//! read past the end, a function that ran off its end). A field is
//! live when every one of those roads is closed:
//!
//! - its declared type is non-nullable and pointer-shaped;
//! - the constructor writes it in the leading run of statements the
//!   instance has not escaped from (`ast::fresh_this`), from a value
//!   that is live by construction;
//! - every other write to a member of that name, anywhere in the
//!   program, stores a value live by construction, and no computed key
//!   store, `delete`, dynamic `new` or reflection surface can reach a
//!   field; a property of an object literal counts as such a write,
//!   since the checker lets a literal of the class's shape into a slot
//!   typed as the class. The one literal left out is the class's own
//!   factory seed for a field the constructor prefix writes, when the
//!   factory passes the seed straight into the constructor: the prefix
//!   overwrites that value before anything can read it;
//! - nothing but an instance of the class reaches a slot typed as it:
//!   the class (or an ancestor) has no constructor answering another
//!   object, and no `as` cast names it over a value typed as something
//!   other than the class, a subclass or `any` (a value typed `any`
//!   enters a class slot through a checked cast that refuses a foreign
//!   object);
//! - no foreign receiver can enter a method twin.
//!
//! Live by construction: `new`, an array or object literal, `this`
//! inside a constructor body (a method's receiver comes from its
//! caller), a closure, a `const` every declaration of which is
//! initialised with one, a call to a declared function all of whose
//! returns are one, which cannot run off its end and whose name no
//! binding shadows, a read of another live field, or a constructor
//! parameter that every `new` and `super(...)` fills with one. Fields
//! and functions are judged together to a greatest fixed point: a
//! cycle (a function returning its own call, a field copied from
//! itself) is optimistic, which is sound because a non-live value can
//! only enter through a leaf shape, and that leaf fails on its own.
//!
//! The same fixed point answers reads of bindings and call results
//! (`bindings`, `scope`), which the lowering guards unless proven, and
//! the parameters a caller tests before the call (`entry`). A builtin
//! answering a fresh object, and a string a literal, a concatenation or
//! a string builtin answers, are live by construction too (`fresh`).
//!
//! The census is syntactic and over-approximates every road, like
//! `arr_head_census`; the cost of a false negative is the guard the
//! program paid before.

mod bindings;
mod collect;
mod decls;
mod disc;
mod entry;
mod fresh;
mod judge;
mod reads;
mod scope;
mod value;
pub(super) mod walk;

pub(crate) use decls::program_live;

use std::collections::HashMap as StdMap;

use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::Type;
use crate::num_width::WidthTable;
use crate::ssa::id_hash::{HashMap, HashSet};

use super::arr_head_census::ExprMask;

/// `(class, field)` pairs whose reads never see the undefined cell.
pub(crate) type LiveFields = HashSet<(String, String)>;

/// What the census proved live.
#[derive(Default)]
pub(crate) struct Live {
    pub(crate) fields: LiveFields,
    /// Reads of a binding and calls whose value the census did not
    /// prove live (`bindings`).
    unproven: ExprMask,
    /// Member reads a discriminant test proved live (`disc`).
    pub(crate) members: ExprMask,
    /// The arguments each direct call tests before the call (`entry`).
    entry: Entry,
}

/// Argument indices, by direct call.
type Entry = HashMap<ExprId, Vec<usize>>;

const THIS: &str = "__this";
/// Member / identifier / string names that reach a field or an
/// instance by other means than a typed read or write.
const ROADS: &[&str] = &[
    "Reflect",
    "eval",
    "Function",
    "Proxy",
    "assign",
    "defineProperty",
    "defineProperties",
    "create",
    "setPrototypeOf",
    "__proto__",
];

struct Ctor<'a> {
    /// The constructor function's own name.
    name: &'a str,
    class: &'a str,
    params: &'a [Param],
    /// The parameters a `new` fills (`walk::user_params`).
    user: Vec<&'a Param>,
    body: &'a [Stmt],
}

/// One prefix write `this.f = v` of a constructor, with the ctor
/// parameter it copies when `v` is a bare parameter.
struct PrefixWrite<'a> {
    class: &'a str,
    field: &'a str,
    value: ExprId,
    param: Option<usize>,
}

struct Census<'a> {
    ast: &'a Ast,
    /// Each expression's checked type, by arena slot.
    types: Vec<Option<&'a Type>>,
    widths: &'a WidthTable,
    /// The specialization each retargeted generic call enters.
    retargets: &'a StdMap<ExprId, String>,
    /// Some road no typed census closes was found: nothing is live.
    road: bool,
    ctors: Vec<Ctor<'a>>,
    fns: HashMap<&'a str, (&'a [Param], &'a [Stmt])>,
    /// Every `let` / `const` initialiser, by name.
    let_inits: HashMap<&'a str, Vec<ExprId>>,
    /// Names bound other than by `let` / `const` or a judged parameter
    /// (other params, loop variables, catch params): a read of one
    /// cannot be judged.
    bound: HashSet<&'a str>,
    /// The constructor parameters a name is bound by, as `(class,
    /// user parameter index)`: a read of one is judged by the class's
    /// construction sites.
    ctor_params: HashMap<&'a str, Vec<(&'a str, usize)>>,
    /// The same for parameters of declared functions judged by their
    /// call sites (`bindings::judged_fn`), as `(function, index)`.
    fn_params: HashMap<&'a str, Vec<(&'a str, usize)>>,
    calls: bindings::Calls<'a>,
    scopes: scope::Scopes<'a>,
    /// Names assigned to anywhere, or declared more than once as a
    /// function.
    assigned: HashSet<&'a str>,
    /// The values `n = v` stores, by name.
    assign_values: HashMap<&'a str, Vec<ExprId>>,
    /// Every `x.f = v` outside the constructor prefixes, by field.
    member_writes: HashMap<&'a str, Vec<ExprId>>,
    /// Field names some `delete x.f` names.
    deleted: HashSet<&'a str>,
    /// Field names written through a receiver not typed as a class.
    foreign: HashSet<&'a str>,
    /// Classes a value other than an instance may be typed as.
    foreign_classes: HashSet<&'a str>,
    /// The expressions of the constructor bodies, where `this` is the
    /// instance under construction.
    ctor_exprs: ExprMask,
    prefix: Vec<PrefixWrite<'a>>,
    /// `new C(..)` and `super(..)` argument lists, by class.
    construct_args: HashMap<&'a str, Vec<&'a [ExprId]>>,
    /// Factory seed literals and the fields of each that the
    /// constructor prefix overwrites unread.
    unread_seeds: HashMap<ExprId, HashSet<&'a str>>,
}
