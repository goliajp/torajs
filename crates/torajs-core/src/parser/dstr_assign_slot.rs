//! The single-slot half of the destructuring-assignment desugar —
//! how one `(target, loaded)` pair becomes a plain assignment (or a
//! nested-pattern recursion), including the §13.15.1 target-validity
//! rejections and the slot-position yield recovery. Split verbatim
//! out of `dstr_assign.rs` (which keeps the pattern walk) at the
//! 500-line cap; the parent answers "how does a pattern expand into
//! slots", this file answers "how does one slot land".

use super::*;

impl<'a> Parser<'a> {
    /// One pattern slot: a simple target gets a direct assign; a
    /// nested pattern hoists the loaded value into a fresh temp and
    /// recurses; anything else is the spec's early error.
    pub(super) fn emit_dstr_assign_slot(
        &mut self,
        target: ExprId,
        loaded: ExprId,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // `0, { yield } = {}` — the shorthand hoisted to a `__yx_`
        // temp, which is not a valid assignment target (§13.15.1).
        self.reject_invalid_assignment_target(target)?;
        // A yield inside the TARGET (`[x[yield]] = v`) — §13.15.5.3
        // step 1 evaluates the target reference at its own slot,
        // before that slot's GetV. Re-emitting the recovered
        // YieldInto here restores that order (the hoist had moved it
        // in front of the whole statement, which the eval-order guard
        // then rejected against the desugar's own synthesized loads).
        // A NESTED PATTERN target recurses through here and its
        // leaves recover for themselves — recovering at the pattern
        // level would strip the buffer before the leaf asks (the
        // rest-nested `[...[x[yield]]]` double-recovery).
        let is_pattern = matches!(
            self.ast.get_expr(target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        );
        if !is_pattern && let Some(recovered) = self.recover_yield_temps(target)? {
            out.extend(recovered);
        }
        // §13.15.1 — `eval` / `arguments` are not valid simple
        // assignment targets in strict code (module code always is),
        // and `yield` is a strict-mode reserved word outright
        // (rotation 346 — `0, { yield } = {}` must reject at parse
        // phase; the checker-side admits landed this rotation let it
        // fall through to a runtime unknown-ident instead).
        if let Expr::Ident(n) = self.ast.get_expr(target)
            && (n == "arguments" || n == "eval" || n == "yield")
        {
            return Err(format!(
                "`{n}` is not a valid assignment target in a destructuring pattern at {} \
                 (ES §13.15.1)",
                self.at()
            ));
        }
        // §12.7.2 ReservedWord — never a valid IdentifierReference,
        // so never an assignment target. The objlit COVER parse
        // admits these as property names (`{ default: x }` is fine)
        // and the shorthand spelling synthesizes an Ident value
        // carrying the keyword text; only the re-read as an
        // AssignmentPattern sees the difference (`{ default } = o`,
        // the t262 syntax-error-ident-ref family), so the parse-phase
        // reject lives here. Contextual keywords (type / async /
        // await / let) stay out — they ARE valid identifier
        // references. A user-written ident can never collide: these
        // spellings lex to their keyword tokens, so an `Ident` node
        // carrying one only ever comes from the shorthand synthesis
        // (escaped spellings included).
        const RESERVED: [&str; 36] = [
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "function",
            "if",
            "import",
            "in",
            "instanceof",
            "new",
            "null",
            "return",
            "super",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "var",
            "void",
            "while",
            "with",
        ];
        if let Expr::Ident(n) = self.ast.get_expr(target)
            && RESERVED.contains(&n.as_str())
        {
            return Err(format!(
                "`{n}` is a reserved word and not a valid assignment target in a \
                 destructuring pattern at {} (ES §13.15.1)",
                self.at()
            ));
        }
        let is_simple = matches!(
            self.ast.get_expr(target),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if is_simple {
            let assign = self.ast.add_expr(Expr::Assign {
                target,
                value: loaded,
            });
            out.push(Stmt::Expr(assign));
            return Ok(());
        }
        if matches!(
            self.ast.get_expr(target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        ) {
            let id = self.mint_desugar_id();
            let tmp = format!("__dstra_src_{id}");
            self.note_ary_destr_group(target, loaded);
            // No annotation (r454, replacing the r453 in-generator
            // `any` pin): when a leaf yield puts this temp across a
            // suspension point the state-machine lift asks the
            // field-annotation sniff, whose FALLBACK for
            // `__dstra_src_*` is `any` — same outcome for this
            // element-load init (the sniff has no answer for it),
            // but a sniffable source keeps its typed lane. See
            // desugar_generators_walkers.
            out.push(Stmt::LetDecl {
                mutable: false,
                name: tmp.clone(),
                type_ann: None,
                init: loaded,
                is_var: false,
            });
            return self.emit_dstr_assign_pattern(target, &tmp, out);
        }
        Err(format!(
            "invalid destructuring assignment target at {}",
            self.at()
        ))
    }
}
