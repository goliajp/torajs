//! for-of loop-variable + destructuring-pattern binding scan, split
//! from `try_parse_for_of` (rotation 119 chunk 6, fn-debt decomp).
//!
//! Body verbatim from the pre-split fn — three destructuring
//! branches (array pattern / object pattern / bare Ident) resolve to
//! a `var_name` (a fresh `__forof_destr_N` or `__forvar_N` for the
//! synthetic loop-locals, or the user's Ident), plus an
//! `assign_target` for the `var` / bare-head assign-form wrap.

use super::*;

use super::forof_destr::ForOfPatScan;

impl<'a> Parser<'a> {
    /// Scan the loop-variable position: destructuring pattern (array
    /// `[a,b]` / object `{x,y}`) → `__forof_destr_N` fresh loop-local
    /// with a body-prepend of per-field lets; bare Ident → the user's
    /// binding name; `var`/bare head → an `__forvar_N` fresh loop-local
    /// with an assign-form body wrap so the user's binding tracks each
    /// iteration. Returns `None` on Bail / unrecognised token so the
    /// caller can restore `pos = saved` and return `Ok(None)`.
    pub(super) fn parse_forof_binding_and_pattern(
        &mut self,
        saved: usize,
        is_var_decl: Option<bool>,
        bare_form: bool,
    ) -> Option<(
        Option<Vec<String>>,
        Option<Vec<(String, String)>>,
        String,
        Option<String>,
    )> {
        // V3-18 wedge — for-of with array-destructuring pattern:
        // `for (let [a, b] of pairs) { ... }`. Common shape for
        // iterating tuple arrays.
        let destruct_names: Option<Vec<String>> = match self.scan_forof_destr_array() {
            ForOfPatScan::Bail => {
                self.pos = saved;
                return None;
            }
            ForOfPatScan::NotPattern => None,
            ForOfPatScan::Pat(names) => Some(names),
        };
        // V3-18 wedge — for-of with object-destructuring pattern:
        // `for (let { x, y } of pts) { ... }`. Mirror of the array
        // destr branch: hoist the iterator variable into a fresh
        // synthetic name (`__forof_destr_<id>`), then prepend
        // per-field `let bound = <iter>.field` lets to the body.
        // Reserved-word fields go through keyword_property_name.
        // Bound binding name still required to be an Ident
        // (reserved-word fields require explicit `field: name`
        // rename — same rule as parse_object_destructuring).
        let destruct_obj: Option<Vec<(String, String)>> = if destruct_names.is_none() {
            match self.scan_forof_destr_obj() {
                ForOfPatScan::Bail => {
                    self.pos = saved;
                    return None;
                }
                ForOfPatScan::NotPattern => None,
                ForOfPatScan::Pat(entries) => Some(entries),
            }
        } else {
            None
        };
        let var_name = if destruct_names.is_some() || destruct_obj.is_some() {
            let id = self.mint_desugar_id();
            format!("__forof_destr_{id}")
        } else {
            match self.peek() {
                Token::Ident(n) => {
                    let nn = n.clone();
                    self.pos += 1;
                    nn
                }
                _ => {
                    self.pos = saved;
                    return None;
                }
            }
        };
        // chunk B2 — `var` / bare forms route through a fresh
        // loop-local; the user's binding is assigned at the top of
        // each iteration (var+destructuring keeps the block-scoped
        // per-field lets — recorded divergence on fn-scope leak).
        let assign_target: Option<String> = if (bare_form || is_var_decl == Some(true))
            && destruct_names.is_none()
            && destruct_obj.is_none()
        {
            Some(var_name.clone())
        } else {
            None
        };
        let var_name = if assign_target.is_some() {
            let id = self.mint_desugar_id();
            format!("__forvar_{id}")
        } else {
            var_name
        };
        Some((destruct_names, destruct_obj, var_name, assign_target))
    }

    /// S2.24 刀 2 (RFC 20260727-dstr-assignment) — bare
    /// assignment-pattern head scan: `for ([a, b] of src)` /
    /// `for ({ x } of src)`. The pattern parses as a literal
    /// expression (the spec's CoverAssignmentPattern, same as the
    /// statement form); a non-of/in follow means this was a C-style
    /// init that happens to open with `[` — the caller restores its
    /// saved position and falls through.
    pub(super) fn scan_forof_assign_pattern(&mut self) -> Option<ExprId> {
        let Ok(pat) = self.parse_expr() else {
            return None;
        };
        let next_is_of_in = matches!(
            self.peek(),
            Token::Ident(n) if n == "of" || n == "in"
        );
        next_is_of_in.then_some(pat)
    }
}
