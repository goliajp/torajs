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
//! (`program_strict_prologue`), and the byte ranges that are strict for
//! another reason (`strict_code_spans` — a `"use strict"` body or the
//! whole source, and, since rotation 618, a class, every part of which
//! is strict code). A declaration is in sloppy code when the goal is
//! script, the program has no prologue, and the declaration's own span
//! falls in none of those ranges.
//!
//! The class ranges matter here for the same reason they matter to the
//! legacy-octal gate next door: §B.3.3 hoisting is written for source
//! text that is not strict mode code, so a `{ function f() {} }` inside
//! a class body does not get it.

use super::annexb_names::annexb_fn_span;
use super::{Expr, ExprId, Stmt};
use std::collections::HashSet;

mod lex_scopes;
mod mint;
pub(super) use lex_scopes::LexScopes;

/// The strictness half of §B.3.3's precondition, snapshotted once per
/// pass so the walk does not carry `&Ast` through every frame.
pub(super) struct AnnexBGoal {
    /// Script goal AND no program-level `"use strict"`.
    sloppy: bool,
    /// Byte ranges that are strict for a reason the goal does not
    /// carry — an explicit directive prologue, or a class.
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
    /// Declarations spliced in from sloppy eval code. Their spans were
    /// blanked at the splice, so the goal and the prologue ranges both
    /// answer for the wrong program; the name is what is left.
    sloppy_eval_names: HashSet<String>,
    /// The same, from strict eval code.
    strict_eval_names: HashSet<String>,
}

impl AnnexBGoal {
    fn of(ast: &super::Ast) -> Self {
        Self {
            sloppy: ast.program_is_sloppy(),
            strict_spans: ast.strict_code_spans.clone(),
            async_names: ast
                .async_fns
                .iter()
                .chain(ast.async_generator_fns.iter())
                .cloned()
                .collect(),
            sloppy_eval_names: ast.sloppy_eval_fn_names.clone(),
            strict_eval_names: ast.strict_eval_fn_names.clone(),
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
    ///
    /// Unless the declaration came out of sloppy eval code, in which
    /// case neither reading applies to it: the goal describes the FILE,
    /// and `(0, 0)` is the very sentinel the splice stamped on. Those
    /// are answered by name. A declaration with a span of its own is
    /// the program's, whatever an eval text declares under its name.
    fn applies(&self, name: &str, span: crate::lexer::Span) -> bool {
        let blanked = span.start == 0 && span.end == 0;
        if blanked && self.sloppy_eval_names.contains(name) {
            return true;
        }
        if blanked && self.strict_eval_names.contains(name) {
            return false;
        }
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
pub(crate) fn annexb_applies_at(ast: &super::Ast, name: &str, span: crate::lexer::Span) -> bool {
    AnnexBGoal::of(ast).applies(name, span)
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
    /// Mangled symbol → the name the declaration was written with,
    /// drained into `Ast::nested_fn_names` on the next flush. Held
    /// here for the same reason `minted` is: the mangle happens while
    /// the walk holds the tree, and the `Ast` is not reachable there.
    pub(super) source_names: Vec<(String, String)>,
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
    /// The parent's own declarations that share their binding with a
    /// written `var` of the same name (`nested_fns_top::var_shared_fn_names`).
    pub(super) var_shared: HashSet<String>,
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
    /// `Ast::eval_annexb_fn_names`, snapshotted: a block function of
    /// eval / Script text whose §B.3.3 `var` is that text's, bound where
    /// the text ran. The `var` this pass mints for such a name is filed
    /// as an eval site at the flush, so `desugar_var_hoist` gives the
    /// name the text's flavour instead of a written `var`'s (which
    /// bound the property in the prelude, non-configurable, and the
    /// program's own later `Object.defineProperty(this, "f", …)` threw).
    eval_annexb: std::collections::BTreeMap<String, super::var_hoist::ScriptGlobalVar>,
    /// The `init` slots minted for those names, drained into
    /// `Ast::eval_var_sites` on the next flush.
    eval_var_inits: Vec<(ExprId, super::var_hoist::ScriptGlobalVar)>,
    /// The lexical scopes between the var scope and the declaration.
    pub(super) lex: LexScopes,
    /// Switches the walk wrapped to give their CaseBlock one binding per
    /// function, drained by the rename half after the flush.
    pub(super) case_blocks: Vec<super::nested_fns_case_block::CaseBlockSlots>,
}

impl LiftCtx {
    pub(super) fn new(ast: &super::Ast) -> Self {
        Self {
            minted: Vec::new(),
            base: ast.exprs.len() as u32,
            counter: 0,
            source_names: Vec::new(),
            goal: AnnexBGoal::of(ast),
            declared: HashSet::new(),
            shadowed: HashSet::new(),
            params: HashSet::new(),
            var_shared: HashSet::new(),
            catch_renames: Vec::new(),
            eval_annexb: ast.eval_annexb_fn_names.clone(),
            eval_var_inits: Vec::new(),
            lex: LexScopes::default(),
            case_blocks: Vec::new(),
        }
    }

    /// Does a BODY-LEVEL declaration of `name` have to become a var
    /// binding of its own? When the same body also declares the name in
    /// a block (sloppy code): the block declaration's §B.3.3 write needs
    /// something to land on, and after the lift the body binds nothing.
    /// And, in any code, when a written `var` of the body shares the
    /// binding (§10.2.11 steps 27-28): the writes need it too.
    /// Answering yes makes the body-level declaration a hoisted
    /// `var name = <mangled>` at the head of the list, which is where a
    /// function declaration's initialization belongs anyway.
    pub(super) fn body_level_var_lift(&self, name: &str, span: crate::lexer::Span) -> bool {
        self.var_shared.contains(name)
            || (self.declared.contains(name) && self.goal.applies(name, span))
    }

    /// Did a sloppy eval splice this declaration in? Its binding is
    /// written where the eval ran (§19.2.1.3 step 17), so the var a lift
    /// mints for it stays at the declaration's own position. The splice
    /// blanked the span to `(0, 0)`, which a declaration the program
    /// wrote never carries.
    pub(super) fn spliced_by_sloppy_eval(&self, name: &str, span: crate::lexer::Span) -> bool {
        span.start == 0 && span.end == 0 && self.goal.sloppy_eval_names.contains(name)
    }

    /// §B.3.3's strictness precondition alone, for the module-top
    /// pre-scan (`nested_fns_top`): is the declaration at `span` in
    /// sloppy code?
    pub(super) fn applies(&self, name: &str, span: crate::lexer::Span) -> bool {
        self.goal.applies(name, span)
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
        if !self.goal.applies(name, span) || self.goal.is_async_form(name) {
            return AnnexB::Strict;
        }
        // §B.3.3.1 skips a name that is also a parameter; §B.3.2.3,
        // its eval-text counterpart, has no such exclusion — the
        // block's write lands on the parameter's binding.
        let param = self.params.contains(name) && !self.spliced_by_sloppy_eval(name, span);
        if param || self.lex.owns(name, span) {
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
        annexb_fn_span(catch_body, p).is_some_and(|span| self.goal.applies(p, span))
    }
}
