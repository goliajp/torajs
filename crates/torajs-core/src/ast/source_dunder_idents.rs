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
//! The reasoning is right; `__` cannot carry it. The prefix is tr's
//! own minting convention, not a reserved namespace, and sputnik's
//! half of test262 writes `__ref`, `__func`, `__key`, `__in__while` as
//! ordinary user identifiers throughout.
//!
//! Provenance carries it, and the boundary is the LEXER, not the
//! arena: an identifier the program wrote arrived as a `Token::Ident`.
//! Reading the expression arena instead would be wrong, because the
//! parser mints too — binary `in` becomes a synthetic
//! `__torajs_in_op(key, obj)` call at parse time
//! (`parser::expr_prec`), and `#x` becomes `__priv_<class>__<x>`.
//! Both are Ident NODES that were never Ident TOKENS, and calling
//! them user code cost 80 conformance cases when this pass first read
//! the arena.
//!
//! Filled from every token stream that feeds the shared Ast — the
//! whole program, each imported module, each direct `eval` source
//! ([`crate::parser::entry`]) and each template interpolation
//! ([`crate::parser::expr_entry`]) — so a `__` user name is recognised
//! wherever it was written.
//!
//! Only `__`-prefixed spellings are kept: the judge asks about no
//! others, and a full ident set would be a per-program allocation for
//! nothing. Reduced pipelines that never parse through these entries
//! leave the set empty, which is the pre-existing posture for every
//! `__` name.

use super::Ast;
use crate::lexer::{Spanned, Token};

pub fn record_source_dunder_idents(ast: &mut Ast, tokens: &[Spanned]) {
    for t in tokens {
        if let Token::Ident(n) = &t.token
            && n.starts_with("__")
            && !ast.source_dunder_idents.contains(n)
        {
            ast.source_dunder_idents.insert(n.clone());
        }
    }
}

/// Is this name one TR minted, rather than one the program spelled?
///
/// The one judge behind every site that used to ask
/// `name.starts_with("__")`: the three undeclared-name checker sites
/// (through `Checker::is_synthesized_dunder`, which reads its own
/// copy of the set) and the lowering's data-global promote gate.
pub fn is_synthesized_dunder(ast: &Ast, name: &str) -> bool {
    name.starts_with("__") && !ast.source_dunder_idents.contains(name)
}
