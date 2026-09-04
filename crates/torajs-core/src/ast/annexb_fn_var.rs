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

use super::{Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// The strictness half of §B.3.3's precondition, snapshotted once per
/// pass so the walk does not carry `&Ast` through every frame.
pub(super) struct AnnexBGoal {
    /// Script goal AND no program-level `"use strict"`.
    sloppy: bool,
    /// Byte ranges made strict by an explicit directive prologue.
    strict_spans: Vec<(u32, u32)>,
}

impl AnnexBGoal {
    fn of(ast: &super::Ast) -> Self {
        Self {
            sloppy: ast.sloppy_script_goal && !ast.program_strict_prologue,
            strict_spans: ast.strict_prologue_spans.clone(),
        }
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
        if !mode.nested || self.shadowed.contains(name) || !self.goal.applies(span) {
            return AnnexB::Legacy;
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

/// The names a statement list declares with a `function` of its own.
/// `Multi` is transparent — its children share the surrounding scope,
/// so a declaration inside one is this list's.
pub(super) fn own_fn_names(body: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    own_fn_names_into(body, &mut out);
    out
}

fn own_fn_names_into(body: &[Stmt], out: &mut HashSet<String>) {
    for st in body {
        match st {
            Stmt::FnDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(inner) => own_fn_names_into(inner, out),
            _ => {}
        }
    }
}

/// The names a statement list's BLOCK-SHAPED children declare with a
/// `function`. Intersected with the list's own declarations it names
/// the collision §B.3.3 cannot otherwise answer: the body-level
/// declaration is the one the var binding should hold before the block
/// runs, and this pass lifts and renames it away.
pub(super) fn block_nested_fn_names(body: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    nested_fn_names_of_list(body, &mut out);
    out
}

/// A statement list that shares its enclosing scope: its OWN
/// declarations are not nested, only what its block-shaped members
/// hold.
fn nested_fn_names_of_list(body: &[Stmt], out: &mut HashSet<String>) {
    for st in body {
        if matches!(st, Stmt::FnDecl { .. }) {
            continue;
        }
        nested_fn_names_of_stmt(st, out);
    }
}

fn nested_fn_names_of_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        // A declaration reached AS a child is nested by construction —
        // the bare-branch form `if (c) function f() {}`.
        Stmt::FnDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::Block(b) => {
            for st in b {
                nested_fn_names_of_stmt(st, out);
            }
        }
        // Transparent — see `nested_fn_names_of_list`.
        Stmt::Multi(b) => nested_fn_names_of_list(b, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            nested_fn_names_of_stmt(then_branch, out);
            if let Some(eb) = else_branch {
                nested_fn_names_of_stmt(eb, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => nested_fn_names_of_stmt(body, out),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body
                .iter()
                .chain(catch_body)
                .chain(finally_body.iter().flatten())
            {
                nested_fn_names_of_stmt(st, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for st in cases
                .iter()
                .flat_map(|c| c.body.iter())
                .chain(default.iter().flatten())
            {
                nested_fn_names_of_stmt(st, out);
            }
        }
        _ => {}
    }
}

/// A function's parameter names. §B.3.3.1 step 1.a.iii skips Annex B
/// when the name is one of them: the parameter already binds it, and
/// the tests say the binding keeps its argument across the
/// declaration.
pub(super) fn param_names(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

/// The names a statement list declares lexically. A `class` is already
/// a `TypeDecl` plus declarations by the time this pass runs, and a
/// `function` is the thing §B.3.3 is about rather than a bar to it, so
/// `let` / `const` are all that is left. `Multi` shares the surrounding
/// scope, so its children count as this list's.
fn lex_names_into(body: &[Stmt], out: &mut Vec<String>) {
    for st in body {
        match st {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } => out.push(name.clone()),
            Stmt::Multi(inner) => lex_names_into(inner, out),
            _ => {}
        }
    }
}

/// The span of a `function <name>` declaration the list gives the
/// §B.3.3 shape. Unlike [`block_nested_fn_names`] the list's OWN
/// declarations count: the only caller passes a catch block, which is
/// a block, so a declaration written directly in it is nested already.
fn annexb_fn_span(body: &[Stmt], name: &str) -> Option<crate::lexer::Span> {
    body.iter().find_map(|st| annexb_fn_span_of_stmt(st, name))
}

fn annexb_fn_span_of_stmt(stmt: &Stmt, name: &str) -> Option<crate::lexer::Span> {
    match stmt {
        Stmt::FnDecl { name: n, span, .. } if n == name => Some(*span),
        Stmt::Block(b) | Stmt::Multi(b) => annexb_fn_span(b, name),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => annexb_fn_span_of_stmt(then_branch, name).or_else(|| {
            else_branch
                .as_deref()
                .and_then(|eb| annexb_fn_span_of_stmt(eb, name))
        }),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => annexb_fn_span_of_stmt(body, name),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => annexb_fn_span(body, name)
            .or_else(|| annexb_fn_span(catch_body, name))
            .or_else(|| {
                finally_body
                    .as_deref()
                    .and_then(|fb| annexb_fn_span(fb, name))
            }),
        Stmt::Switch { cases, default, .. } => cases
            .iter()
            .find_map(|c| annexb_fn_span(&c.body, name))
            .or_else(|| default.as_deref().and_then(|d| annexb_fn_span(d, name))),
        _ => None,
    }
}
