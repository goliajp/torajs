//! Class-heritage helpers (RFC 20260815-heritage-exprid).
//!
//! A `ClassDecl`'s `parent` is an `ExprId` per §15.7 (the heritage is a
//! LeftHandSideExpression). Every static consumer — class_index,
//! class_parents, super rewrites, hoist admission — keys on a NAME, so
//! they all read the heritage through `parent_ident_name`: the answer is
//! `Some(name)` exactly when the heritage is a bare identifier, which is
//! the only shape those static paths can (and did) handle. A non-Ident
//! heritage answers `None` and is routed to the value-shaped-parent lane
//! instead (RFC knife 2).

use super::{Ast, Expr, ExprId};

impl Ast {
    /// The statically-known parent-class name of a heritage expression:
    /// `Some(name)` iff it is a bare `Expr::Ident`.
    pub fn parent_ident_name(&self, parent: Option<ExprId>) -> Option<&str> {
        match self.get_expr(parent?) {
            Expr::Ident(n) => Some(n.as_str()),
            _ => None,
        }
    }

    /// Extract every non-Ident heritage expression to a `let
    /// __ccp<N>: any = <expr>` binding minted right before the class
    /// (RFC 20260815 knife 2a) — §15.7.14 evaluates the heritage at
    /// class-definition time, and the binding position preserves that
    /// order. The heritage then points at `Ident("__ccp<N>")`, and the
    /// name is recorded in `es5_value_parents` so the capturing lane
    /// admits the class as a value-shaped-parent subclass (its ES5
    /// machinery is already runtime-shaped: `Object.create(P.prototype)`
    /// + `P.call(this, …)`).
    ///
    /// `extends null` (§15.7: legal, [[Prototype]] chain null, no
    /// super) extracts too since rotation 410 — the minted name joins
    /// `es5_null_parents`, and the lane's proto link reads that set to
    /// emit `Object.create(null)` and skip the class-side
    /// `setPrototypeOf` (the class object keeps %Function.prototype%).
    pub(in crate::ast) fn extract_value_heritage(&mut self) {
        // A bare-Ident heritage naming NO class declaration and no
        // subclassable builtin is a VALUE binding (`{ let B = Real;
        // class K extends B }` — 393-04 tail registration), and it
        // extracts too: the minted `__ccp<N>` keeps `es5_value_parents`
        // an all-unique-names set (a user spelling like `B` recurs
        // across scopes), and reading the block-local alias makes the
        // class a CAPTURING one, which is exactly the lane that runs
        // the value-shaped-parent machinery. The census walks the same
        // ground this pass does — stmt spine plus arena arrow bodies.
        let mut counts = std::collections::HashMap::new();
        for s in &self.stmts {
            super::hoist_nested_classes_census::count_class_decl_names(s, &mut counts);
        }
        for e in &self.exprs {
            if let Expr::ArrowFn { body, .. } = e {
                for s in body {
                    super::hoist_nested_classes_census::count_class_decl_names(s, &mut counts);
                }
            }
        }
        let class_names: std::collections::HashSet<String> = counts.into_keys().collect();
        let mut counter: u32 = 0;
        let mut stmts = std::mem::take(&mut self.stmts);
        extract_in_list(self, &mut stmts, &mut counter, &class_names);
        for s in stmts.iter_mut() {
            super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |list| {
                extract_in_list(self, list, &mut counter, &class_names);
            });
        }
        self.stmts = stmts;
        // Fn-expression / arrow bodies live in the expr arena; nested
        // arrows are separate arena entries, so the flat scan covers
        // deep nesting (hoist_nested_classes does the same walk). The
        // arena grows as bindings mint — length re-read per step.
        let mut i = 0;
        while i < self.exprs.len() {
            let mut body = match &mut self.exprs[i] {
                Expr::ArrowFn { body, .. } => std::mem::take(body),
                _ => {
                    i += 1;
                    continue;
                }
            };
            extract_in_list(self, &mut body, &mut counter, &class_names);
            for s in body.iter_mut() {
                super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |list| {
                    extract_in_list(self, list, &mut counter, &class_names);
                });
            }
            if let Expr::ArrowFn { body: b, .. } = &mut self.exprs[i] {
                *b = body;
            }
            i += 1;
        }
    }

    /// Overwrite a discarded heritage node with a name-free literal.
    /// Passes that CONSUME a ClassDecl (the capturing lane's ES5
    /// rewrite, the static lane's TypeDecl+FnDecl split) drop the Stmt
    /// but not the arena node — and several analyses scan the whole
    /// arena (the fnexpr_this use-shape census among them), where an
    /// orphaned bare `Expr::Ident` reads as a USE of that binding.
    /// Rotation 409 measured the damage: the orphan disqualified every
    /// ES5-lane parent binding from `this`-promotion, demoting
    /// `__this` from parameter to capture. A String parent vanished
    /// with the Stmt; an ExprId must be tombstoned explicitly.
    pub fn tombstone_expr(&mut self, id: ExprId) {
        self.exprs[id.0 as usize] = Expr::Null;
    }
}

/// One statement list: insert the minted binding right before each
/// class whose heritage extracts. `for_each_nested_vec_mut` recurses
/// the spine, so this handles a single flat list.
fn extract_in_list(
    ast: &mut Ast,
    list: &mut Vec<super::Stmt>,
    counter: &mut u32,
    class_names: &std::collections::HashSet<String>,
) {
    let mut i = 0;
    while i < list.len() {
        match extract_one(ast, &mut list[i], counter, class_names) {
            Some(binding) => {
                list.insert(i, binding);
                i += 2;
            }
            None => i += 1,
        }
    }
}

/// The class shapes a heritage hangs off: a bare ClassDecl or one
/// inside an `export` wrapper.
fn extract_one(
    ast: &mut Ast,
    s: &mut super::Stmt,
    counter: &mut u32,
    class_names: &std::collections::HashSet<String>,
) -> Option<super::Stmt> {
    use super::Stmt;
    let class = match s {
        Stmt::ClassDecl { .. } => s,
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => inner.as_mut(),
        _ => return None,
    };
    let Stmt::ClassDecl { parent, .. } = class else {
        return None;
    };
    let pid = (*parent)?;
    let mut is_null = false;
    match ast.get_expr(pid) {
        // `extends null` (§15.7: legal) extracts too — the minted
        // binding rides the value lane like any heritage expression,
        // and `es5_null_parents` tells the proto link to spell
        // `Object.create(null)` (reading `null.prototype` would throw
        // at class-definition time, which the spec does not).
        Expr::Null => is_null = true,
        // A NAMED heritage stays put when the name means something to a
        // static path: a class declaration (the static/capturing name
        // lanes), a subclassable builtin (`strip_builtin_heritage`
        // recognises it by name AFTER this pass), a binding this pass
        // already minted, or `undefined` (a shape no lane claims —
        // extracting it would only trade one loud error for another).
        Expr::Ident(n) => {
            if class_names.contains(n)
                || super::desugar_classes_builtin_heritage::is_subclassable_builtin(n)
                || ast.es5_value_parents.contains(n)
                || n == "undefined"
            {
                return None;
            }
        }
        _ => {}
    }
    let name = format!("__ccp{}", *counter);
    *counter += 1;
    let alias = ast.add_expr(Expr::Ident(name.clone()));
    *parent = Some(alias);
    ast.es5_value_parents.insert(name.clone());
    if is_null {
        ast.es5_null_parents.insert(name.clone());
    }
    Some(Stmt::LetDecl {
        mutable: false,
        name,
        type_ann: Some("any".to_string()),
        init: pid,
        is_var: false,
    })
}
