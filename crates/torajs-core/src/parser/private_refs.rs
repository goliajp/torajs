//! Deferred private-name reference resolution (ES §15.7 lexical
//! scoping).
//!
//! A `#x` reference inside a class body binds to the INNERMOST
//! enclosing class that declares `#x` — nested classes see outer
//! names, an inner redeclaration shadows. A single forward pass
//! cannot decide the declaring class at the reference token (later
//! members of the same body may still declare `#x`), so
//! `member_name_after_dot` parses every reference to a
//! `__privu_<site>__<raw>` placeholder and parks `(raw name, scope-id
//! stack)` in `ast.private_ref_sites`. This pass runs at the end of
//! `parse_into` — every declaration of the file is in — and rewrites
//! each placeholder to the declaring class's `__priv_<C>__<raw>`
//! member name, the exact string declaration-side mangling baked into
//! `struct_layouts`, so everything downstream (checker visibility,
//! static rewrite tables, runtime by-name dispatch) is untouched.
//!
//! No declaring scope on the stack → fall back to the INNERMOST
//! class. That reproduces the pre-pass single-`current_class` mangle
//! byte-for-byte, and the checker's undeclared-private compile-time
//! reject (`check_type_of_member.rs` `__priv_` branch) stays the
//! error surface for `this.#nope`.

use super::*;

impl Parser<'_> {
    /// Rewrite every `__privu_<site>__<raw>` member name minted while
    /// parsing exprs `from..` to its resolved `__priv_<C>__<raw>`
    /// form. `from` scopes the walk to this file's slice of the arena
    /// (imported files resolved their own slice already; placeholders
    /// never survive a `parse_into`).
    pub(super) fn resolve_private_refs(&mut self, from: u32) -> Result<(), String> {
        if self.ast.private_ref_sites.is_empty() {
            return Ok(());
        }
        for i in (from as usize)..self.ast.exprs.len() {
            // `#x in o` keys ride as a String literal inside the
            // synthetic `__torajs_priv_in_op` Call — the placeholder
            // is only rewritten when it round-trips against a parked
            // site (index in range AND the raw name matches), so an
            // ordinary user string can't be caught by accident.
            let (name, is_in_key) = match &self.ast.exprs[i] {
                Expr::Member { name, .. } | Expr::OptChain { name, .. } => (name.clone(), false),
                Expr::String(s) if s.starts_with("__privu_") => (s.to_string_lossy_owned(), true),
                _ => continue,
            };
            let Some(rest) = name.strip_prefix("__privu_") else {
                continue;
            };
            let Some((site, site_raw)) = rest.split_once("__") else {
                continue;
            };
            let Ok(site) = site.parse::<usize>() else {
                continue;
            };
            let Some((raw, stack)) = self.ast.private_ref_sites.get(site) else {
                continue;
            };
            if site_raw != raw {
                continue;
            }
            let declaring = stack
                .iter()
                .find(|&&sid| self.ast.class_private_scopes[sid as usize].1.contains(raw))
                .copied();
            match declaring {
                Some(sid) => {
                    let cls = &self.ast.class_private_scopes[sid as usize].0;
                    let resolved = format!("__priv_{cls}__{raw}");
                    match &mut self.ast.exprs[i] {
                        Expr::Member { name, .. } | Expr::OptChain { name, .. } => *name = resolved,
                        Expr::String(s) => *s = resolved.into(),
                        _ => unreachable!(),
                    }
                }
                // §13.10 / §13.1 early error — an in-key with no
                // declaring scope has nothing to brand-check against
                // (`class C { #a; m(o) { return #b in o; } }`).
                None if is_in_key => {
                    return Err(format!(
                        "private name `#{raw}` is not declared in any enclosing class"
                    ));
                }
                // A member reference inside a class body that no
                // enclosing class declares is the same §13.1 early
                // error the in-key branch raises — AllPrivateNamesValid
                // is false, and the clause says SyntaxError at PARSE.
                //
                // It used to be left to the checker, which reads the
                // mangled name off a `ClassRef`-typed receiver. That
                // made a parse-phase early error depend on the STATIC
                // TYPE of `this`, and rotation 399 walked straight into
                // it: promoting a function expression's receiver types
                // `__this` as `any`, the `ClassRef` arm stops matching,
                // and `class C { f = function () { this.#x } }` — a
                // test262 negative case — went from correctly refused
                // to silently accepted. Deciding it here is both the
                // spec phase and the only reading that no downstream
                // typing can erase.
                // An EMPTY stack is the same error, not an exemption:
                // §13.1 AllPrivateNamesValid holds no private names at
                // all outside every class, and the one real producer of
                // class-free sites is a class HERITAGE (parsed in the
                // outer private environment since rotation 409 —
                // `class C extends function () { this.#foo } {}` must
                // die at parse; the branch used to skip these and the
                // program evaluated).
                None => {
                    return Err(format!(
                        "private name `#{raw}` is not declared in any enclosing class"
                    ));
                }
            }
        }
        Ok(())
    }
}
