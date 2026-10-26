//! The statements a lift mints into the scope it lifts from — the
//! §B.3.3 `var`, its write, the block binding — and the arena slot each
//! one's expression takes. Moved out of the parent when it neared the
//! file-size limit; the parent keeps the eligibility questions.

use super::LiftCtx;
use crate::ast::{Expr, ExprId, Stmt};

impl LiftCtx {
    /// Mint `var <name>: any = <mangled>` for the declaration's own
    /// textual position. `desugar_var_hoist` — which runs after this
    /// pass — splits it into the scope-top `undefined` binding and this
    /// assignment, which is exactly §B.3.3.1's shape: the var binding is
    /// created uninitialized-to-`undefined` on scope entry and written
    /// when the declaration is *evaluated*.
    pub(in crate::ast) fn annexb_var_decl(&mut self, name: &str, mangled: &str) -> Stmt {
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
    pub(in crate::ast) fn mint_block_decl(&mut self, name: &str, mangled: &str) -> Stmt {
        let init = self.mint(Expr::Ident(mangled.to_string()));
        Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: false,
        }
    }

    /// `var <name>: any;` and `<name> = <mangled>` — the pair the var
    /// hoist makes of `var <name>: any = <mangled>`, minted apart for
    /// the head of a module (`nested_fns_top`). Init-less and `any` on
    /// purpose: initialized with the function value, the hoist keeps
    /// the `var` as a lexical `let` typed by that value (its escape
    /// hatch), and unannotated, the checker types the slot by its one
    /// kind of writer — either way a TYPED slot, which the global
    /// object never sees. `any` is the slot the global object binds
    /// (`emit_script_global_var_bind`), and it is the width a `var` of
    /// the same name in a block shares (§14.3.2.1: one binding).
    pub(in crate::ast) fn mint_var_slot_and_write(
        &mut self,
        name: &str,
        mangled: &str,
    ) -> [Stmt; 2] {
        let init = self.mint(Expr::Uninit);
        let slot = Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: true,
        };
        [slot, self.mint_write(name, mangled)]
    }

    /// `var <name>: any = <mangled>` — the binding starts here.
    pub(in crate::ast) fn mint_var_decl(&mut self, name: &str, mangled: &str) -> Stmt {
        let init = self.mint(Expr::Ident(mangled.to_string()));
        if let Some(flavor) = self.eval_annexb.get(name) {
            self.eval_var_inits.push((init, *flavor));
        }
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

    /// Move what has been minted into the arena and re-base.
    ///
    /// Must run before anything READS an id this pass handed out. The
    /// rewrite half does: it walks the very statements the lift just
    /// wrote, `var f: any = <mangled>` among them, and an id past the
    /// end of the arena is an `index out of bounds` there — reached
    /// only when one parent scope has both an Annex B declaration and a
    /// legacy one, since the rewrite is skipped when every name is
    /// block-scoped.
    pub(in crate::ast) fn flush(&mut self, ast: &mut crate::ast::Ast) {
        // Through `add_expr`, never `exprs.append`: the arena has a
        // parallel span vector, and pushing to one without the other
        // desynchronizes them. The ids stay the ones `mint` handed out
        // because `add_expr` numbers from `exprs.len()`, which is
        // `base` here.
        for e in std::mem::take(&mut self.minted) {
            ast.add_expr(e);
        }
        for (mangled, mut source) in std::mem::take(&mut self.source_names) {
            // an async declaration that suspends over a frame keeps
            // doing so under its lifted name
            if ast.async_ssa.fns.contains(&source) {
                ast.async_ssa.fns.insert(mangled.clone());
            }
            // a body `function arguments` renamed apart from the
            // parameter-scope object still answers its own spelling
            if source == crate::ast::apply_args_materialize::body_arguments::BODY_ARGUMENTS {
                source = "arguments".to_string();
            }
            // likewise one renamed out of a default's way, or apart
            // from a same-named declaration of another kind
            if let Some(shown) = ast.renamed_binding_display.get(&source) {
                source = shown.clone();
            }
            ast.nested_fn_names.insert(mangled, source);
        }
        for (init, flavor) in std::mem::take(&mut self.eval_var_inits) {
            ast.eval_var_sites.insert(init, flavor);
        }
        self.base = ast.exprs.len() as u32;
    }
}
