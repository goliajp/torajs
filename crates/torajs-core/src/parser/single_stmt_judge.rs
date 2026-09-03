//! The single-statement-position judge (rotation 578).
//!
//! §14.6 IfStatement, §14.7 IterationStatement and §14.13
//! LabelledStatement all take a `Statement`, so a Declaration in any
//! of those bodies is an early error. One judge answers for all
//! seven positions, and [`SingleStmtPos`] is what lets it give them
//! different answers where the grammar does.
//!
//! Moved out of the `loops` sibling verbatim: it lived there because
//! `parse_while` was its first caller, but that module's own doc
//! names only the four iteration / branching parsers, and the judge
//! is asked from `parser.rs` and two other siblings besides.

use super::*;

/// Which position handed its body to
/// [`Parser::reject_decl_in_single_stmt`]. The judge has to ask,
/// because the Annex B extensions it must honour belong to two
/// DIFFERENT positions — and to no others.
///
/// §14.6 IfStatement, §14.7 IterationStatement and §14.13
/// LabelledStatement all take a `Statement`, and a
/// FunctionDeclaration is not one. Annex B then adds it back in
/// exactly two places: §B.3.2 lets a LabelledItem BE a
/// FunctionDeclaration, and §B.3.4 gives IfStatement four extra
/// productions with FunctionDeclaration branches. Nothing extends a
/// loop body, so `while (false) function f(){}` is a SyntaxError
/// under every goal — which is what bun answers, and what tr ran
/// happily until the judge could tell the positions apart.
pub(super) enum SingleStmtPos {
    /// A labelled statement's own body — §B.3.2's position. It is the
    /// only one where a label chain around a function IS the
    /// extension rather than something wrapping it, so
    /// `l1: l2: function f(){}` stays legal as a statement-list item.
    LabelledItem,
    /// An `if` / `else` branch — §B.3.4's position. The bare spelling
    /// is admitted; §14.13.1 still refuses the labelled one, because
    /// B.3.4 extends `if (x) function f(){}` and never
    /// `if (x) l: function f(){}` (rotation 577 刀 4).
    IfBranch,
    /// A loop body: `while` / `do`-`while` / `for` / `for`-`in` /
    /// `for`-`of`. No extension reaches here, so both spellings go.
    LoopBody,
}

impl Parser<'_> {
    /// §14.6 / §14.7 / §14.13 early error — a single-statement
    /// position (if/else branch, loop body, labeled body) admits a
    /// Statement, not a Declaration. Lexical declarations (let /
    /// const / class / generator / async fn) reject, and `var` is a
    /// VariableStatement proper. The check is post-parse on the
    /// produced Stmt — parse side effects are moot since the whole
    /// compile aborts.
    ///
    /// A plain `function` is the one answer that differs BY POSITION,
    /// which is what [`SingleStmtPos`] carries: Annex B hands the
    /// production back in exactly two places and to no others.
    pub(super) fn reject_decl_in_single_stmt(
        &self,
        body: &Stmt,
        body_start: usize,
        ctx: &str,
        pos: SingleStmtPos,
    ) -> Result<(), String> {
        if !matches!(pos, SingleStmtPos::LabelledItem)
            && let Some(kind) = Self::labelled_fn_kind(body)
        {
            return Err(format!(
                "a labeled {kind} is not allowed as the body of {ctx} at {} \
                 (ES §14.13.1)",
                self.at()
            ));
        }
        let offending = match body {
            Stmt::LetDecl {
                is_var: false,
                mutable,
                ..
            } => {
                // Sloppy-mode `let \n x = 1` is an ASI-split
                // ExpressionStatement pair (`let` the identifier),
                // not a LexicalDeclaration — the §13.16 lookahead
                // only forbids `let [`. tr misparses the shape into
                // one LetDecl; rejecting it here turned a
                // runs-fine program into a parse error (test262
                // let-identifier-with-newline family), so the
                // newline spelling keeps the historical behavior.
                if *mutable && self.let_newline_asi_form(body_start) {
                    None
                } else {
                    Some(if *mutable { "let" } else { "const" })
                }
            }
            // RFC 20260809 knife 3 residue — §14.7/§14.13 take a
            // Statement; a UsingDeclaration (single or the Multi a
            // multi-binding head parses to) is not one
            // (with-initializer-for/do/while-statement family).
            Stmt::UsingDecl { .. } => Some("using"),
            Stmt::Multi(inner) if inner.iter().any(|s| matches!(s, Stmt::UsingDecl { .. })) => {
                Some("using")
            }
            Stmt::ClassDecl { .. } => Some("class"),
            Stmt::FnDecl {
                name, is_generator, ..
            } => {
                if *is_generator {
                    Some("generator function")
                } else if self.ast.async_fns.contains(name) {
                    Some("async function")
                } else if matches!(pos, SingleStmtPos::LoopBody) {
                    // The half no extension covers. §14.7's body is a
                    // Statement under every goal, so this one refuses
                    // in sloppy code too — unlike the strict-only
                    // halves of B.3.2 / B.3.4.
                    Some("function")
                } else {
                    None
                }
            }
            _ => None,
        };
        match offending {
            Some(kind) => Err(format!(
                "{kind} declarations are not allowed as the body of {ctx} at {}",
                self.at()
            )),
            None => Ok(()),
        }
    }

    /// §14.13.1 IsLabelledFunction — walk a label chain and answer
    /// what it wraps, when that is a function declaration. The chain
    /// may be any depth (`l1: l2: l3: function f(){}`), which is why
    /// this recurses rather than peeking one level.
    fn labelled_fn_kind(body: &Stmt) -> Option<&'static str> {
        let Stmt::Labeled { body, .. } = body else {
            return None;
        };
        match &**body {
            Stmt::FnDecl { .. } => Some("function declaration"),
            inner => Self::labelled_fn_kind(inner),
        }
    }

    /// `let` at `body_start` followed by a LINE-BREAK then an
    /// identifier — the ASI-identifier spelling the reject above
    /// exempts. (`var` shares `Token::Let` at the token level but
    /// its LetDecl carries `is_var: true` and never reaches this.)
    pub(super) fn let_newline_asi_form(&self, body_start: usize) -> bool {
        let (Some(a), Some(b)) = (self.tokens.get(body_start), self.tokens.get(body_start + 1))
        else {
            return false;
        };
        matches!(a.token, Token::Let)
            && matches!(b.token, Token::Ident(_))
            && self.source[a.span.end as usize..b.span.start as usize].contains('\n')
    }
}
