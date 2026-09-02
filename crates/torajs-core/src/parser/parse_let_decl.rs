//! `let` / `var` / `const` declaration statement parsing — split
//! from `parse_stmt.rs` (rotation 422: the expression-position
//! `yield*` arm pushed that file past its 500-line cap). Two
//! private members ride along: `parse_let_decl_stmt` (the only
//! caller is parse_stmt_dispatch) and its class-value alias
//! registrar.

use super::*;

/// The annotation a declaration with NO initializer carries:
/// `T | undefined`, in the parser's `__nullable(T)` spelling
/// (567-04).
///
/// `let x: T;` binds nothing until its first write, and at runtime
/// that nothing is `undefined`: bun prints it, while tr rejected the
/// whole program (`declared Number, init has Undefined`) or, for a
/// pointer-shaped T, read the type's zero value. The annotation that
/// states the truth is the one the user could have written by hand,
/// and writing it HERE is what makes every later consumer agree
/// without any of them learning about the uninitialized case: the
/// checker face, the slot type each `try_resolve_type_ann` caller
/// picks, and the per-type undefined sentinel the nullable slot
/// already carries for `f(a?: T)`.
///
/// What TS reports for `let x: number; use(x)` comes from
/// definite-assignment analysis, which is a different question from
/// what the slot holds — and not one a runtime may answer by
/// refusing to run. `let x!: T` asserts the answer to that question
/// and has no runtime face, so it is wrapped the same way.
///
/// A T that already admits undefined, or already admits everything,
/// is returned unchanged.
fn no_init_type_ann(t: String) -> String {
    match t.as_str() {
        "any" | "undefined" | "null" => t,
        _ if t.starts_with("__nullable(") => t,
        _ => format!("__nullable({t})"),
    }
}

impl<'a> Parser<'a> {
    /// `let` / `var` / `const` declaration statement (multi-decl,
    /// destructuring dispatch, `= yield` J.4 shape) — split from
    /// `parse_stmt` (2026-07-03, fn-debt decomp). Body verbatim,
    /// dedented one level.
    pub(super) fn parse_let_decl_stmt(
        &mut self,
        mutable: bool,
        is_var: bool,
    ) -> Result<Stmt, String> {
        let kw = if is_var {
            "var"
        } else if mutable {
            "let"
        } else {
            "const"
        };
        self.pos += 1;
        // Destructuring: `let [a, b] = src` or `let { x, y } = src`.
        // Parsed inline so it shares the let-decl's lookahead. Both
        // forms desugar to `let __t = src; let <field>...; ...` so the
        // backend never sees a destructuring pattern.
        if matches!(self.peek(), Token::LBracket | Token::LBrace) {
            // `is_var` travels with `mutable`. Dropping it made every
            // `var { a } = src` bind block-scoped, so `{ var { a } =
            // src } a` answered `unknown identifier` where §14.3.2
            // hoists the binding to the enclosing function — and
            // inside a `with` body the same mistake made the name
            // SHADOW the object, which a `var` never does.
            return self.parse_destructuring_decl(mutable, is_var);
        }
        // V3-18 m1.h.5 — multi-decl `let a, b = 1, c` per spec
        // §14.3.1. Each binding can have its own type ann and
        // optional init; commas separate; final semi closes.
        // Decls are emitted as a Stmt::Multi so subsequent
        // passes see them as a flat statement sequence.
        let mut decls: Vec<Stmt> = Vec::new();
        loop {
            let name = match self.peek() {
                Token::Ident(n) => {
                    let n = n.clone();
                    // §12.7.2 / §13.1.1 — `static` and friends, plus
                    // `eval` and `arguments`, are ordinary identifiers
                    // in sloppy code and refused in strict;
                    // per-function strictness rejects here, the goal
                    // half is recorded for the prelude gate.
                    self.note_strict_binding(&n, self.in_strict_fn)?;
                    n
                }
                // §12.7.2 — `let/var/const yield` is a valid binding
                // wherever the predicate still admits the name; the
                // recorded site is what the strict-GOAL gate raises on.
                Token::Yield if self.yield_reads_as_ident() => {
                    let at = self.at();
                    self.ast.yield_ident_positions.push(at);
                    "yield".to_string()
                }
                // §12.7.2 / §14.3.1.1 — `var let = 1` is an ordinary
                // sloppy declaration, but `let let` and `const let`
                // are Syntax Errors even in sloppy code, so only the
                // `var` spelling asks the predicate. The other two
                // fall through to the reject below, which is what
                // the spec wants there.
                Token::Let if kw == "var" && self.let_reads_as_ident() => {
                    self.record_strict_goal_site("let");
                    "let".to_string()
                }
                t => {
                    return Err(format!(
                        "expected identifier after `{kw}`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            // 563-07 — TS's definite assignment assertion, `let x!: T`.
            // It is a claim addressed to the type checker ("this IS
            // assigned before any read") with no runtime face and no
            // effect on the declared type, so the parse just steps
            // over it. TS additionally requires an annotation and
            // forbids an initializer here; those early errors are a
            // recorded boundary, not enforced.
            if matches!(self.peek(), Token::Bang) {
                self.pos += 1;
            }
            let type_ann = if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                Some(self.parse_type_ann()?)
            } else {
                None
            };
            // No-init shape: `let x` / `let x: T` (followed by
            // `,` or `;` or — per JS ASI — a known statement-
            // start token on the next line). Const requires an
            // init by spec. T-37-followup-asi: accept Switch /
            // If / For / While / Try / Function / Class / Let /
            // Const / Var / Return / Throw / Break / Continue /
            // Do / RBrace as ASI-implied terminators so test262
            // patterns like `let x\nswitch (x) {...}` parse.
            let next_is_stmt_start = matches!(
                self.peek(),
                Token::Switch
                    | Token::If
                    | Token::For
                    | Token::While
                    | Token::Try
                    | Token::Function
                    | Token::Class
                    | Token::Let
                    | Token::Const
                    | Token::Var
                    | Token::Return
                    | Token::Throw
                    | Token::Break
                    | Token::Continue
                    | Token::Do
                    | Token::RBrace
            );
            if matches!(self.peek(), Token::Semi | Token::Comma) || next_is_stmt_start {
                if !mutable {
                    return Err(format!(
                        "`const {name}` requires an initializer at {}",
                        self.at()
                    ));
                }
                let init = self.ast.add_expr(Expr::Uninit);
                let type_ann = type_ann.map(no_init_type_ann);
                decls.push(Stmt::LetDecl {
                    mutable,
                    name,
                    type_ann,
                    init,
                    is_var,
                });
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                // Only consume Semi as terminator; for ASI-style
                // stmt-start, leave the token for the outer parse.
                if matches!(self.peek(), Token::Semi) {
                    self.pos += 1;
                }
                break;
            }
            match self.peek() {
                Token::Eq => self.pos += 1,
                t => return Err(format!("expected `=`, got {t:?} at {}", self.at())),
            }
            // J.4 — `let name(:T)? = yield ...` (plain and `yield*`)
            // — see parse_let_yield_init below.
            if decls.is_empty() && matches!(self.peek(), Token::Yield) && self.in_generator {
                return self.parse_let_yield_init(mutable, is_var, name, type_ann);
            }
            let init = self.parse_expr()?;
            let init = match self.expand_dstr_assign_init(init, &mut decls)? {
                Some(rhs_temp) => rhs_temp,
                None => {
                    self.register_class_value_alias(&name, init);
                    init
                }
            };
            decls.push(Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            });
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                continue;
            }
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            break;
        }
        return Ok(if decls.len() == 1 {
            decls.into_iter().next().unwrap()
        } else {
            Stmt::Multi(decls)
        });
    }

    /// §13.15.2 — a destructuring assignment in the INIT position
    /// (`var y = { p: x } = src`): the value of a destructuring
    /// assignment is the RHS itself, so the pattern assignments run
    /// first (pushed onto `decls` through the shared
    /// `desugar_dstr_assign`) and the declared binding reads back the
    /// hoisted src temp — `Some(temp)` replaces the init. The
    /// statement lane (expr_stmt_or_dstr_assign) never sees this
    /// shape: the Assign is an init subexpression.
    fn expand_dstr_assign_init(
        &mut self,
        init: ExprId,
        decls: &mut Vec<Stmt>,
    ) -> Result<Option<ExprId>, String> {
        let Expr::Assign { target, value } = self.ast.get_expr(init) else {
            return Ok(None);
        };
        if !matches!(
            self.ast.get_expr(*target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        ) {
            return Ok(None);
        }
        let (t, v) = (*target, *value);
        // Rotation 455 — hoist the RHS once BEFORE the pattern expand
        // (the chain lane's shape), and read the declared binding back
        // from THAT temp, not from the pattern's group temp: the group
        // temp may be materialized through the iterator lane as a NEW
        // Array<Any>, and §13.15.2 says the assignment's value is the
        // RHS reference itself (`var r = ([x] = vals); r === vals`).
        let id = self.mint_desugar_id();
        let chain_name = format!("__dstra_chain_{id}");
        decls.push(Stmt::LetDecl {
            mutable: false,
            name: chain_name.clone(),
            type_ann: None,
            init: v,
            is_var: false,
        });
        let chain_ref = self.ast.add_expr(Expr::Ident(chain_name.clone()));
        let stmts = self.desugar_dstr_assign(t, chain_ref)?;
        decls.extend(stmts);
        Ok(Some(self.ast.add_expr(Expr::Ident(chain_name))))
    }

    /// J.4 — `let name(:T)? = yield <expr>?;` / `= yield* src;` init.
    /// Only valid as a single-decl (the multi-decl middle falls
    /// through to parse_expr, which rejects yield — v0.5 generator
    /// semantics). Outside a generator the init falls through to
    /// parse_expr, where primary.rs admits `yield` as an
    /// IdentifierReference (§12.7.2 goal triage). Caller has peeked
    /// `Token::Yield` inside a generator and NOT consumed it.
    fn parse_let_yield_init(
        &mut self,
        mutable: bool,
        is_var: bool,
        name: String,
        type_ann: Option<String>,
    ) -> Result<Stmt, String> {
        self.pos += 1;
        // `let v = yield* src;` — the delegation's done value binds v
        // (§27.5.3.2). Same hoist as the assignment form
        // (parse_yield.rs): the buf drains in front of this
        // statement, so the init reads the settled temp.
        if matches!(self.peek(), Token::Star) {
            self.pos += 1;
            let src = self.parse_expr()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            let init = self.emit_yieldstar_expr_hoist(src);
            return Ok(Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            });
        }
        // S2.41 — `let v = yield;` binds the resumption value with an
        // undefined operand (same optional-operand rule as the
        // statement lane).
        let value = self.parse_yield_operand()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::YieldInto {
            var: name,
            type_ann,
            value,
        })
    }

    /// P8.5 — narrow-surface class-value alias registration for a
    /// just-parsed `<kw> name = init`. Peek the init expr:
    ///   (i) `const F = class { ... }` → init is the synth Ident
    ///       emitted by parse_primary's Class branch
    ///       (`__ClassExpr_<id>`). Register F → that name.
    ///   (ii) `const G = F` where F is already an alias → propagate so
    ///        G also maps to the underlying synth class.
    /// The map is read by parse_new (`new F()` → the synth class's
    /// static factory) and by parse_postfix's Dot arm (`F.method(...)`
    /// → the synth class's static-method machinery). RC-3 (RFC
    /// 20260706-test262-bug-corpus): let/var bindings register too —
    /// the map is linear parse-order (not scoped), so any later
    /// rebinding or reassignment of the name drops the alias and falls
    /// back to the dynamic path instead of silently binding the old
    /// class.
    /// ES §8.4 NamedEvaluation — an anonymous class expression takes
    /// the name of the binding it is assigned to. The parser has
    /// already replaced the expression with its `__ClassExpr_<id>`
    /// synth Ident, so a naming position records the user spelling
    /// against that synth and `class_display_name` reads it back for
    /// `.name`, the class-object print and the instance prefix.
    /// First naming wins — a later alias of the same class does not
    /// rename it. A class expression with its OWN BindingIdentifier
    /// (`class Named {}`) never reaches here: the parser keeps that
    /// name, and there is no synth to override.
    pub(super) fn name_anonymous_class_expr(&mut self, name: &str, value: ExprId) {
        let Expr::Ident(synth) = self.ast.get_expr(value) else {
            return;
        };
        if !synth.starts_with("__ClassExpr_") {
            return;
        }
        let synth = synth.clone();
        self.ast
            .class_expr_display_names
            .entry(synth)
            .or_insert_with(|| name.to_string());
    }

    fn register_class_value_alias(&mut self, name: &str, init: ExprId) {
        let mut aliased = false;
        if let Expr::Ident(init_name) = self.ast.get_expr(init) {
            if init_name.starts_with("__ClassExpr_") {
                self.class_value_aliases
                    .insert(name.to_string(), init_name.clone());
                // RFC 20260714-dstr-residual blade 4 — ES §8.4
                // NamedEvaluation: `let D = class {}` names the
                // anonymous class expression by its binding.
                self.name_anonymous_class_expr(name, init);
                aliased = true;
            } else if let Some(target) = self.class_value_aliases.get(init_name) {
                let target = target.clone();
                self.class_value_aliases.insert(name.to_string(), target);
                aliased = true;
            }
        }
        if !aliased {
            self.class_value_aliases.remove(name);
        }
    }
}
