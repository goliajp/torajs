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
        // §13.15.1 — a pattern slot is an early error when the
        // AssignmentTargetType of its target is not simple, and
        // §13.1.3 makes that judgement STRICTNESS-DEPENDENT: `eval`
        // and `arguments` are invalid targets in strict code and
        // perfectly ordinary names outside it, `yield` is only an
        // IdentifierReference at all where it is neither reserved nor
        // a YieldExpression. This site used to refuse all three
        // unconditionally, which cost the 17 `*-no-strict` t262 cases
        // that assign to exactly these names on purpose.
        //
        // It has to judge by NAME rather than let the admission sites
        // decide, because the object-literal shorthand reaches here
        // having gone through the PROPERTY-name reader: that reader
        // accepts the word and turns it into a string, and only the
        // re-read as an AssignmentPattern asks whether the string is
        // a legal reference (`strict_reserved`'s module doc records
        // the same asymmetry for `{ let }`). So both halves of the
        // strictness answer are needed here — the per-function half
        // now, and the per-goal half parked for the prelude gate,
        // whose §13.1.3 message already names this position.
        if let Expr::Ident(n) = self.ast.get_expr(target) {
            let n = n.clone();
            if n == "yield" {
                if !self.yield_reads_as_ident() {
                    return Err(format!(
                        "`yield` is not a valid assignment target in a destructuring \
                         pattern at {} (ES §13.15.1)",
                        self.at()
                    ));
                }
                let at = self.at();
                self.ast.yield_ident_positions.push(at);
            } else {
                self.note_strict_binding(&n, self.strict_here())?;
            }
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
            // §13.15.5.5 is recursive: a nested pattern whose OWN
            // elements carry Member / Index targets owes them the same
            // `lref -> step -> put` interleaving the outer one gets, so
            // it needs a group of its own -- park slot, prefix-0 walk
            // and close wrap included -- rather than the plain index
            // reads below. Its close wrap nests inside the outer
            // pattern's, which is what §7.4.9 asks for: the inner
            // iterator settles before the outer one hears about the
            // abrupt completion.
            if self.detect_stepwise(target) {
                return self.emit_dstr_assign_group(target, loaded, out);
            }
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
