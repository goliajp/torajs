//! Annex B §B.3.3 eligibility — does a block-nested function
//! declaration also get a `var`-scoped binding?
//!
//! §B.3.3 gives a function declaration nested in a block TWO bindings:
//! the block-scoped one every mode has, and a var-scoped one on the
//! nearest function/global VariableEnvironment. Only the second is
//! Annex B, and it exists only in SLOPPY code — a strict function body
//! (or a strict program) keeps the block binding alone.
//!
//! tr carries three readings of strictness after the parse, all landed
//! by rotation 579: the goal (`sloppy_script_goal`, keyed on the `.cts`
//! extension), the program-level directive prologue
//! (`program_strict_prologue`), and the byte ranges an explicit
//! `"use strict"` prologue made strict (`strict_prologue_spans` — a
//! function body, or the whole source). A declaration is in sloppy code
//! when the goal is script, the program has no prologue, and the
//! declaration's own span falls in none of those ranges.

use super::annexb_names::{annexb_fn_span, lex_names_into};
use super::{Expr, ExprId, Stmt};
use std::collections::HashSet;

/// The strictness half of §B.3.3's precondition, snapshotted once per
/// pass so the walk does not carry `&Ast` through every frame.
pub(super) struct AnnexBGoal {
    /// Script goal AND no program-level `"use strict"`.
    sloppy: bool,
    /// Byte ranges made strict by an explicit directive prologue.
    strict_spans: Vec<(u32, u32)>,
    /// §B.3.3 is written for a `FunctionDeclaration`. A
    /// `GeneratorDeclaration`, an `AsyncFunctionDeclaration` and an
    /// `AsyncGeneratorDeclaration` are none of it, in sloppy code as
    /// much as in strict — `{ async function y() {} } typeof y`
    /// answers "undefined" in a plain sloppy script (node, measured).
    ///
    /// Async-ness is name-keyed here because that is how this codebase
    /// carries it: `Ast::async_fns` exists precisely so an `is_async`
    /// flag need not be added to every `FnDecl` construction site, and
    /// `desugar_async` reads it the same way.
    async_names: HashSet<String>,
}

impl AnnexBGoal {
    fn of(ast: &super::Ast) -> Self {
        Self {
            sloppy: ast.sloppy_script_goal && !ast.program_strict_prologue,
            strict_spans: ast.strict_prologue_spans.clone(),
            async_names: ast
                .async_fns
                .iter()
                .chain(ast.async_generator_fns.iter())
                .cloned()
                .collect(),
        }
    }

    /// Is this one of the declaration forms §B.3.3 does not reach?
    fn is_async_form(&self, name: &str) -> bool {
        self.async_names.contains(name)
    }

    /// Does the declaration at `span` get the Annex B var binding?
    /// A synthesized declaration carries the `(0, 0)` sentinel — it has
    /// no user source, so no directive prologue can cover it, and the
    /// goal alone decides.
    fn applies(&self, span: crate::lexer::Span) -> bool {
        self.sloppy
            && !self
                .strict_spans
                .iter()
                .any(|&(a, b)| span.start >= a && span.start < b)
    }
}

/// §B.3.3's strictness precondition, for the one caller outside this
/// walk: [`super::nested_fns_capture`] routes a capturing declaration
/// off the lifting lane before the lift ever sees it, and has to ask the
/// same question about the block form it leaves behind.
pub(super) fn annexb_applies_at(ast: &super::Ast, span: crate::lexer::Span) -> bool {
    AnnexBGoal::of(ast).applies(span)
}

/// What §B.3.3 says about one declaration.
pub(super) enum AnnexB {
    /// It applies: the declaration also writes a var binding, and the
    /// outer references belong to that binding rather than to the
    /// lifted block one.
    Apply,
    /// It does not apply, because something else in this scope owns the
    /// name — an enclosing `let` / `const`, or a parameter. Those outer
    /// references belong to that owner, so they must not be redirected
    /// to the lifted name either.
    Skip,
    /// The code is strict, so §B.3.3 does not exist for it and the
    /// declaration is what §14.2.10 says it is: a binding of the BLOCK.
    /// Nothing outside the block binds the name, so outer references
    /// must not be redirected to the lifted one — the same treatment
    /// [`AnnexB::Skip`] gives them, for the other reason.
    Strict,
    /// It does not apply, and nothing else claims the name: strict
    /// code, or a declaration this pass itself renames away so that no
    /// binding is left to write. The lift answers as it did before
    /// §B.3.3 was implemented.
    Legacy,
}

/// Where in the scope tree the walk currently stands. §B.3.3 is about
/// a function declaration nested in a BLOCK; a declaration written
/// directly in a function body is an ordinary hoisted declaration with
/// one binding, and taking it for the Annex B shape gives it a second
/// one that shadows it.
#[derive(Clone, Copy)]
pub(super) struct LiftMode {
    /// The walk is inside a function body rather than at module top.
    /// Only it makes a bare-branch declaration block-scoped for outer
    /// references — the strict half of the S5.7 boundary decision.
    pub(super) in_fn_body: bool,
    /// The walk has descended into a block-shaped statement, so a
    /// declaration found here is the Annex B shape.
    pub(super) nested: bool,
}

impl LiftMode {
    pub(super) fn descend(self) -> Self {
        Self {
            nested: true,
            ..self
        }
    }
}

/// The state a lift needs that is the same for every parent scope:
/// the expression arena a §B.3.3 var declaration mints its initializer
/// in, the mangling counter (unique across the whole pass), and the
/// strictness snapshot that decides whether the var binding exists at
/// all. Bundled so the walk keeps its arity.
pub(super) struct LiftCtx {
    /// Expressions this pass mints, appended to the arena when the pass
    /// ends. Held apart from the `Ast` because the rewrite half of the
    /// pass borrows the whole `Ast` between lifts; nothing in that half
    /// mints an expression, so the ids handed out against `base` stay
    /// accurate until the flush.
    minted: Vec<Expr>,
    base: u32,
    pub(super) counter: u32,
    goal: AnnexBGoal,
    /// Names the CURRENT parent scope already binds — its own top-level
    /// function declarations and its parameters. §B.3.3.1 step 3.a.ii.1
    /// creates and zero-initializes the var binding only when
    /// `varEnv.HasBinding(F)` is false; when it is true the Annex B
    /// write still happens but must not reset the existing binding to
    /// `undefined` first. Reassigned before each parent's walk. A
    /// same-name `var` needs no entry: `desugar_var_hoist` already
    /// shares one binding across every `var` of a name in a scope.
    pub(super) declared: HashSet<String>,
    /// Names the current parent scope declares with a `function` of its
    /// OWN — a body-level declaration this pass lifts and renames away.
    /// After the lift the scope no longer binds the name at all, so
    /// neither shape of the §B.3.3 write has anything to land on: a
    /// fresh `var` would not be the hoisted declaration the name is
    /// supposed to hold before the block, and a bare assignment would
    /// have no slot. Annex B is skipped for these, and the pass answers
    /// as it did before it existed. Empty at module top, where a
    /// top-level `function f` survives the pass and lands in `declared`.
    pub(super) shadowed: HashSet<String>,
    /// The current parent function's parameter names.
    pub(super) params: HashSet<String>,
    /// Catch parameters this pass renamed away, as `(original, fresh)`.
    /// §B.3.3.1 writes through `varEnv.SetMutableBinding` — the
    /// VariableEnvironment of the enclosing function or script, which a
    /// catch parameter is not part of. tr lowers a catch parameter as a
    /// scope of its own wrapped around the catch block, so a write left
    /// under the original name lands on the parameter and the var
    /// binding outside never moves. Renaming the parameter, and the
    /// references that mean it, hands the name back to the var scope.
    /// Collected here because the walk cannot borrow the `Ast`; the
    /// rewrite half applies them.
    pub(super) catch_renames: Vec<(String, String)>,
    /// Names lexically declared by the scopes between the var scope and
    /// the declaration, innermost last. §B.3.3.1 step 1.a.ii applies
    /// Annex B only when replacing the declaration with `var F` would
    /// raise no early error, and a `let` / `const` of the same name in
    /// any enclosing block is exactly that error — so a hit here skips
    /// the whole thing, var binding included. Pushed and truncated by
    /// the walk, so it is a stack, not a set.
    lex: Vec<String>,
}

impl LiftCtx {
    pub(super) fn new(ast: &super::Ast) -> Self {
        Self {
            minted: Vec::new(),
            base: ast.exprs.len() as u32,
            counter: 0,
            goal: AnnexBGoal::of(ast),
            declared: HashSet::new(),
            shadowed: HashSet::new(),
            params: HashSet::new(),
            catch_renames: Vec::new(),
            lex: Vec::new(),
        }
    }

    /// Does a BODY-LEVEL declaration of `name` have to become a var
    /// binding of its own? Only when the same body also declares the
    /// name in a block: the block declaration's §B.3.3 write needs
    /// something to land on, and after the lift the body binds nothing.
    /// Answering yes makes the body-level declaration a hoisted
    /// `var name = <mangled>` at the head of the list, which is where a
    /// function declaration's initialization belongs anyway.
    pub(super) fn body_level_var_lift(&self, name: &str, span: crate::lexer::Span) -> bool {
        self.declared.contains(name) && self.goal.applies(span)
    }

    /// Does the declaration at `span` get the §B.3.3 var binding?
    pub(super) fn annexb(&self, name: &str, span: crate::lexer::Span, mode: LiftMode) -> AnnexB {
        // Two things used to answer `Legacy` together and mean
        // opposite things. A declaration that is not block-nested is
        // an ordinary declaration of this body and keeps the name;
        // one that is block-nested in STRICT code binds its block and
        // nothing else. Reading the second as the first is what let
        // `{ function s() {} } typeof s` answer "function" where the
        // spec — and bun — say "undefined".
        if !mode.nested || self.shadowed.contains(name) {
            return AnnexB::Legacy;
        }
        if !self.goal.applies(span) || self.goal.is_async_form(name) {
            return AnnexB::Strict;
        }
        if self.params.contains(name) || self.lex.iter().any(|n| n == name) {
            return AnnexB::Skip;
        }
        AnnexB::Apply
    }

    /// Does this catch parameter stand between a §B.3.3 write and the
    /// var binding the write is addressed to? Only when the catch block
    /// itself — a block, so a declaration at its top level is already
    /// the Annex B shape — or a block inside it declares a function of
    /// the parameter's name, in sloppy code. §B.3.5 already permits the
    /// collision; what is wrong without the rename is only which of the
    /// two bindings the write reaches.
    pub(super) fn catch_param_shadows_write(&self, p: &str, catch_body: &[Stmt]) -> bool {
        annexb_fn_span(catch_body, p).is_some_and(|span| self.goal.applies(span))
    }

    /// Enter a scope: remember the stack height, then record what the
    /// statement list declares lexically. The mark goes back to
    /// [`Self::leave_scope`] when the walk leaves.
    pub(super) fn enter_scope(&mut self, body: &[Stmt]) -> usize {
        let mark = self.lex.len();
        lex_names_into(body, &mut self.lex);
        mark
    }

    /// Enter a scope that binds exactly one name — a `for` head's
    /// `let` / `const`, or a for-in / for-of element binding.
    pub(super) fn enter_binding(&mut self, name: &str) -> usize {
        let mark = self.lex.len();
        self.lex.push(name.to_string());
        mark
    }

    /// A scope that binds nothing — the mark alone, so both arms of a
    /// loop head can leave the same way.
    pub(super) fn enter_binding_none(&self) -> usize {
        self.lex.len()
    }

    pub(super) fn leave_scope(&mut self, mark: usize) {
        self.lex.truncate(mark);
    }

    /// Start a fresh var scope. Nothing outside a function body's own
    /// lexical scopes can raise the early error §B.3.3.1 asks about.
    pub(super) fn reset_scopes(&mut self) {
        self.lex.clear();
    }

    /// Move what has been minted into the arena and re-base.
    ///
    /// Must run before anything READS an id this pass handed out. The
    /// rewrite half does: it walks the very statements the lift just
    /// wrote, `var f: any = <mangled>` among them, and an id past the
    /// end of the arena is an `index out of bounds` there — reached
    /// only when one parent scope has both an Annex B declaration and a
    /// legacy one, since the rewrite is skipped when every name is
    /// block-scoped.
    pub(super) fn flush(&mut self, ast: &mut super::Ast) {
        // Through `add_expr`, never `exprs.append`: the arena has a
        // parallel span vector, and pushing to one without the other
        // desynchronizes them. The ids stay the ones `mint` handed out
        // because `add_expr` numbers from `exprs.len()`, which is
        // `base` here.
        for e in std::mem::take(&mut self.minted) {
            ast.add_expr(e);
        }
        self.base = ast.exprs.len() as u32;
    }

    /// Mint `var <name>: any = <mangled>` for the declaration's own
    /// textual position. `desugar_var_hoist` — which runs after this
    /// pass — splits it into the scope-top `undefined` binding and this
    /// assignment, which is exactly §B.3.3.1's shape: the var binding is
    /// created uninitialized-to-`undefined` on scope entry and written
    /// when the declaration is *evaluated*.
    pub(super) fn annexb_var_decl(&mut self, name: &str, mangled: &str) -> Stmt {
        if self.declared.contains(name) {
            return self.mint_write(name, mangled);
        }
        self.mint_var_decl(name, mangled)
    }

    /// `<name> = <mangled>` — the scope binds the name already.
    /// The scope binds this name already (its own `function f`, or a
    /// parameter). Write it, do not re-declare it — a hoisted
    /// `let f: any = <Uninit>` would shadow the existing binding and
    /// make it read `undefined` before the block.
    fn mint_write(&mut self, name: &str, mangled: &str) -> Stmt {
        let target = self.mint(Expr::Ident(name.to_string()));
        let value = self.mint(Expr::Ident(mangled.to_string()));
        let assign = self.mint(Expr::Assign { target, value });
        Stmt::Expr(assign)
    }

    /// `let <name>: any = <mangled>` — the binding is the BLOCK's, for
    /// strict code where §B.3.3 adds nothing. It goes to the head of
    /// the block's own statement list, because a function declaration
    /// is hoisted WITHIN its block: a call written above it works, and
    /// so does mutual recursion between two of them.
    ///
    /// `let`, not `var`: a `var` would be hoisted out to the enclosing
    /// function by `desugar_var_hoist`, which is the very leak this
    /// binding exists to stop.
    pub(super) fn mint_block_decl(&mut self, name: &str, mangled: &str) -> Stmt {
        let init = self.mint(Expr::Ident(mangled.to_string()));
        Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: false,
        }
    }

    /// `var <name>: any = <mangled>` — the binding starts here.
    pub(super) fn mint_var_decl(&mut self, name: &str, mangled: &str) -> Stmt {
        let init = self.mint(Expr::Ident(mangled.to_string()));
        Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: true,
        }
    }

    fn mint(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.base + self.minted.len() as u32);
        self.minted.push(e);
        id
    }
}
