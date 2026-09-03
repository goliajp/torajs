//! The `__`-prefixed identifier spellings the PROGRAM TEXT contains.
//!
//! Three checker sites — the undeclared read
//! ([`crate::check_type_of_ident`]), the undeclared write
//! ([`crate::check_assign_ident`]) and the unresolved capture
//! ([`crate::check_type_of_fn`]) — give an unresolvable name the
//! §6.2.5.5 / §6.2.5.6 posture (types `Any`, one deduped warning, a
//! catchable `ReferenceError` when evaluated) rather than a compile
//! reject. Each kept one carve-out that stayed a hard error: a
//! `__`-prefixed name, on the reasoning that the compiler synthesizes
//! those, so an unresolved one is a compiler bug rather than user code.
//!
//! `__` is not a reserved namespace in JavaScript. It is tr's own
//! convention, and the prefix cannot tell a name tr minted from a name
//! the program spelled — sputnik's half of test262 writes `__ref`,
//! `__func`, `__key`, `__in__while` as ordinary user identifiers
//! throughout, and every one of them hit the reject.
//!
//! The question the carve-out actually wants answered is provenance,
//! and provenance is knowable exactly: an identifier the parser
//! produced came from the text. So snapshot the `__`-prefixed
//! identifier spellings out of the expression arena while the arena
//! still holds nothing but what the parser put there — a name in this
//! set is user code, a `__` name absent from it is tr's own.
//!
//! Runs first in the prelude, before `desugar_eval`: every later pass
//! mints names, and that one is the earliest that does (`__evcv0`,
//! `__dynfn_*`, `__evargs_*`). The cost of taking it there is that a
//! `__`-prefixed user name introduced BY an eval'd source string is
//! not in the set and keeps the old reject — a recorded remainder,
//! not a regression, and one the eval desugar can close by inserting
//! the names of each source it parses.
//!
//! Reduced pipelines (REPL, LSP) that never call this leave the set
//! empty, which is the pre-existing posture for every `__` name.
//!
//! Only `__`-prefixed spellings are recorded — the judge asks about no
//! others, and a full ident set would be a per-program allocation for
//! nothing.

use super::{Ast, Expr};

pub fn record_source_dunder_idents(ast: &mut Ast) {
    let names: std::collections::HashSet<String> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Ident(n) if n.starts_with("__") => Some(n.clone()),
            _ => None,
        })
        .collect();
    ast.source_dunder_idents.extend(names);
}
