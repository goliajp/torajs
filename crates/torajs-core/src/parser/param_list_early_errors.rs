//! Param-list early-error checks — split from `param_list.rs` when
//! the untyped-rest implicit-`any[]` wedge pushed it past the
//! 500-line limit (the rotation-227 watch predicted exactly this
//! move). Three §15 early errors shared by every parameter-list
//! parser; the doc on each records the measured test262 cost of
//! refusing more eagerly than the spec does.

use super::*;

impl<'a> Parser<'a> {
    /// A duplicate parameter name is a SyntaxError, and *where* decides
    /// whether unconditionally:
    ///
    /// - A **method definition** (§15.4) and an **arrow** (§15.3.1) take
    ///   `UniqueFormalParameters`, so duplicates are refused whatever
    ///   the list looks like. `unique = true`.
    /// - A **function declaration or expression** takes plain
    ///   `FormalParameters`, whose §15.1.1 early error applies only in
    ///   strict code or when the list is not simple. In sloppy code with
    ///   a simple list `function f(a, a) {}` is legal, and test262
    ///   asserts it: `param-duplicated-non-strict.js` (×2) and
    ///   `S10.2.1_A2.js` run it and expect it to work. Refusing there
    ///   cost exactly those three cases — measured, not predicted.
    ///
    /// The strict half is NOT decided here. A function's own
    /// `"use strict"` sits inside the body its parameters precede, so
    /// at this point the answer is not knowable — and a class body,
    /// the second source, was the only one this ever asked about.
    /// `Parser::judge_duplicate_params_strict` owns all three sources
    /// at the end of the body, where `reject_strict_reserved_params`
    /// already judges parameter NAMES for the identical reason.
    ///
    /// P-SURF S2.9 — tr used to refuse `*m(x = 0, x)` by accident. The
    /// generator desugar turns parameters into fields of the `__Gen_*`
    /// state-machine class, two same-named parameters became two
    /// same-named fields, and the field-conflict check panicked. That
    /// refusal was right for the wrong reason and said so out loud,
    /// naming a synthesized class the user never wrote; the plain
    /// spelling `function f(x = 0, x) {}` was not refused at all.
    ///
    /// Called from every parameter-list parser rather than from one
    /// shared place, because there is no shared place: `parse_fn` and
    /// the arrow parser each carry their own copy of the loop.
    fn reject_duplicate_params(&self, params: &[Param], unique: bool) -> Result<(), String> {
        // §15.1.2 IsSimpleParameterList — no default, no rest, no
        // binding pattern (which arrives as a synthesized holder name).
        let simple = params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && !p.name.starts_with("__param_destr_"));
        if !unique && simple {
            return Ok(());
        }
        for (i, p) in params.iter().enumerate() {
            if params[..i].iter().any(|q| q.name == p.name) {
                return Err(format!(
                    "duplicate parameter name `{}` at {}",
                    p.name,
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// The end of a formal parameter list: the duplicate check below
    /// plus the one binding effect the parser has to record here.
    ///
    /// RC-3 — a parameter binds its spelling for the whole body that
    /// follows, and the P8.5 class-value alias map is linear
    /// parse-order with no scopes, so an alias standing on that
    /// spelling gets read from inside the body and answers the class
    /// it remembered instead of the argument: `let C = class Inner {
    /// static t = "i" }` followed by `function g(C) { return C.t }`
    /// answered `"i"` whatever was passed. `let` rebinding and
    /// assignment already drop the alias; a parameter is the third
    /// binding form that has to. The drop is permanent and the rest
    /// of the program falls back to the dynamic path — the same trade
    /// the other two make.
    ///
    /// A destructuring parameter binds through its `destr_lets`, not
    /// through `Param::name` (which is a synthesized holder), so it
    /// is not reached from here — recorded, not fixed.
    pub(super) fn finish_formal_params(
        &mut self,
        params: &[Param],
        unique: bool,
    ) -> Result<(), String> {
        self.reject_duplicate_params(params, unique)?;
        for p in params {
            self.class_value_aliases.remove(&p.name);
        }
        Ok(())
    }

    /// ES §14.2.1 early error: a `let` / `const` at the top level of a
    /// function body may not repeat a parameter name. (`var` may — it
    /// is the same variable, per §14.3.2.1 — and so may a nested
    /// function declaration, which is var-scoped.)
    ///
    /// P-SURF S2.9, the other half of [`Self::reject_duplicate_params`]
    /// and refused by the same accident until now: the name collided as
    /// two fields of the generator's `__Gen_*` class, which caught
    /// `*foo(a) { let a = 3 }` and missed the plain spelling.
    ///
    /// `destr_lets` carries the bindings a destructuring parameter
    /// unpacks (`function f({ a }) {}` binds `a`), which are parameter
    /// names for this purpose even though they arrive as statements.
    /// The synthesized `__param_destr_N` holders are not — they are
    /// unspellable, so no user declaration can collide with one.
    pub(super) fn reject_lexical_shadowing_param(
        &self,
        params: &[Param],
        destr_lets: &[Stmt],
        body: &[Stmt],
    ) -> Result<(), String> {
        let mut bound: Vec<&str> = params
            .iter()
            .map(|p| p.name.as_str())
            .filter(|n| !n.starts_with("__param_destr_"))
            .collect();
        for s in destr_lets {
            if let Stmt::LetDecl { name, .. } = s {
                bound.push(name.as_str());
            }
        }
        for s in body {
            let Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } = s
            else {
                continue;
            };
            if bound.contains(&name.as_str()) {
                return Err(format!(
                    "`{name}` is declared in the body and is already a parameter, at {}",
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// ES §15.1.1 (mirrored for every other function form — arrow
    /// §15.3.1, method §15.4.1, generator §15.5.1, async §15.8.1,
    /// async generator §15.6.1, async arrow §15.9.1): it is a Syntax
    /// Error if ContainsUseStrict of the body is true and
    /// IsSimpleParameterList of the parameter list is false. The
    /// directive would retroactively make the parameter initializers
    /// strict code, so the spec refuses the combination outright.
    ///
    /// ContainsUseStrict looks only at this function's own directive
    /// prologue — the leading run of string-literal expression
    /// statements — never into nested bodies, which each get their own
    /// call at their own parse site.
    ///
    /// Precision note: a Use Strict Directive is defined on the *source
    /// text* (`"use strict"` is not one), but by the time the body
    /// reaches us the lexer has cooked escapes away, so this compares
    /// the cooked value. The divergence needs an escaped spelling AND a
    /// non-simple parameter list in the same function — test262 has no
    /// such case (its two escaped-directive cases are simple-param) and
    /// real code has no reason to write one. Single-part template
    /// literals collapse to `Expr::String` in the parser and are
    /// caught here too; same reasoning.
    pub(super) fn reject_use_strict_with_non_simple_params(
        &self,
        params: &[Param],
        body: &[Stmt],
    ) -> Result<(), String> {
        let simple = params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && !p.name.starts_with("__param_destr_"));
        if simple {
            return Ok(());
        }
        for s in body {
            let Stmt::Expr(id) = s else { break };
            let Expr::String(v) = &self.ast.exprs[id.0 as usize] else {
                break;
            };
            if v == "use strict" {
                return Err(format!(
                    "`\"use strict\"` directive in a function with a non-simple parameter list at {}",
                    self.at()
                ));
            }
        }
        Ok(())
    }
}
