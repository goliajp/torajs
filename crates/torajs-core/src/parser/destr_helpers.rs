//! Destructuring-binding helpers for let/const/param positions (chunk 410).
//!
//! Extracted verbatim from parser.rs — the methods that lower TS binding
//! patterns (`[a, b, c]`, `{ x, y }`, `{ x: foo }`) into a synthetic-name
//! + per-element / per-field let synthesis:
//! - emit_object_coercible_guard — §13.3.3.5 null / undefined source
//! - record_dstr_default_name — §8.4.5 NamedEvaluation registry
//! - parse_destr_param — entry point for fn-param destructuring
//! - parse_destr_into — dispatch on `[` vs `{` and delegate
//! - parse_destr_array_into — array-pattern element walker
//! - parse_destr_object_into — object-pattern field walker
//!
//! The `= D` slot initializers live in the `destr_defaults` sibling.
//!
//! All `pub(super)` for cross-module impl-block access. Called only from
//! parser.rs main file (parse_fn / parse_class_member / parse_let /
//! parse_const paths).

use super::*;

impl<'a> Parser<'a> {
    /// ES §13.3.3.5 RequireObjectCoercible — an ObjectBindingPattern
    /// throws TypeError when its source is null / undefined, even for
    /// the empty pattern `{}` (which otherwise reads nothing) and for
    /// nested patterns (whose member reads would otherwise fault on a
    /// null intermediate). One loose-eq compare covers both values:
    /// `if (src == null) throw new TypeError(...)`.
    pub(super) fn emit_object_coercible_guard(&mut self, src_name: &str) -> Stmt {
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let null_e = self.ast.add_expr(Expr::Null);
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::LooseEq,
            left: src_ref,
            right: null_e,
        });
        let msg = self.ast.add_expr(Expr::String(
            "cannot destructure null or undefined".to_string().into(),
        ));
        let err = self.ast.add_expr(Expr::New {
            class_name: "TypeError".to_string(),
            args: vec![msg],
            type_args: vec![],
        });
        Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(vec![Stmt::Throw(err)])),
            else_branch: None,
        }
    }

    /// §13.15.5.4 CopyDataProperties — the rest object itself: every
    /// own enumerable key of `src` the sibling fields did not name.
    ///
    /// It rides as an object literal carrying a `__spread_omit__:`
    /// sentinel field, a shape the object-literal lanes downstream
    /// already expand. Both destructuring forms mint it here so the
    /// declaration and the assignment answer with one construction
    /// rather than two that have to be kept agreeing.
    pub(super) fn emit_obj_rest_expr(
        &mut self,
        src_name: &str,
        omit: &[&str],
        computed: &[String],
    ) -> ExprId {
        if !computed.is_empty() {
            return self.emit_obj_rest_call(src_name, omit, computed);
        }
        let sentinel = format!("__spread_omit__:{}", omit.join(","));
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        self.ast.add_expr(Expr::ObjectLit {
            fields: vec![(PropKey::from(sentinel), src_ref)],
        })
    }

    /// The same rest object when one of the excluded keys is a
    /// COMPUTED one. The sentinel spelling cannot carry it — the omit
    /// list is a comma-separated string of names, and a runtime key
    /// has no name — and the rest object's shape stops being a static
    /// answer with it, so this form says so: `__torajs_obj_rest(src,
    /// "a,b", [k1, k2])`, typed `any`, lowered onto the §7.3.25
    /// kernel with both halves of `excludedItems`.
    ///
    /// `computed` names the `__ck_N` temps, which by now hold the
    /// keys ALREADY put through ToPropertyKey at their own position
    /// in the pattern — so the excluded list and the field load read
    /// one conversion, not two.
    fn emit_obj_rest_call(&mut self, src_name: &str, omit: &[&str], computed: &[String]) -> ExprId {
        let callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_obj_rest".to_string()));
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let names = self.ast.add_expr(Expr::String(omit.join(",").into()));
        let elems: Vec<ExprId> = computed
            .iter()
            .map(|n| self.ast.add_expr(Expr::Ident(n.clone())))
            .collect();
        let keys = self.ast.add_expr(Expr::Array(elems));
        self.ast.add_expr(Expr::Call {
            callee,
            args: vec![src_ref, names, keys],
        })
    }

    /// RFC 20260714-dstr-residual blade 2 — ES §8.4.5 NamedEvaluation
    /// for destructuring defaults: when a binding's default is an
    /// anonymous function definition, record (ArrowFn ExprId →
    /// binding name) so `lift_arrow_fns` can carry the name onto the
    /// lifted `__closure_N` (pass-2B merges it into the `.name`
    /// registry; a named fn expression's self-name still wins).
    pub(super) fn record_dstr_default_name(&mut self, default_eid: ExprId, binding: &str) {
        let mut eid = default_eid;
        loop {
            match self.ast.get_expr(eid) {
                Expr::As { expr, .. } => eid = *expr,
                Expr::ArrowFn { .. } => {
                    self.ast.dstr_default_names.insert(eid, binding.to_string());
                    return;
                }
                // Blade 4 — an anonymous class expression parses into
                // an `Ident(__ClassExpr_<id>)` use site; NamedEvaluation
                // flows through the class display-name channel instead.
                Expr::Ident(n) if n.starts_with("__ClassExpr_") => {
                    let n = n.clone();
                    self.ast
                        .class_expr_display_names
                        .entry(n)
                        .or_insert_with(|| binding.to_string());
                    return;
                }
                _ => return,
            }
        }
    }

    pub(super) fn parse_destr_param(&mut self, lets: &mut Vec<Stmt>) -> Result<String, String> {
        let id = self.mint_desugar_id();
        let synth = format!("__param_destr_{id}");
        self.parse_destr_into(synth.clone(), lets)?;
        Ok(synth)
    }

    /// P-PARSE.2 — recursive split for destructuring patterns of any
    /// nesting depth. Each leaf binding emits a
    /// `let leaf = <src>[i]` (array) or `let leaf = <src>.<field>`
    /// (object) into `lets`; each nested sub-pattern (`[a, [b, c]]`,
    /// `{ x: { y } }`) synthesizes an intermediate
    /// `__nested_destr_<id>` binding and recurses with that as the
    /// new source name. The flat MVP from the v3 wedge cycle becomes
    /// the depth-1 case of this recursion — no behaviour change for
    /// existing fixtures.
    pub(super) fn parse_destr_into(
        &mut self,
        src_name: String,
        lets: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        match self.peek() {
            Token::LBracket => self.parse_destr_array_into(src_name, lets),
            Token::LBrace => self.parse_destr_object_into(src_name, lets),
            t => Err(format!(
                "expected `[` or `{{` to start a destr param, got {t:?} at {}",
                self.at()
            )),
        }
    }

    pub(super) fn parse_destr_array_into(
        &mut self,
        outer_src: String,
        lets: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // assumes current token is `[`
        self.pos += 1;
        // RFC 20260714-dstr-residual blade 3 — every element read goes
        // through a group temp bound to the source rather than through
        // the source name itself. When the source turns out not to be a
        // statically indexable container (a generator, a Map / Set, a
        // class instance with `[Symbol.iterator]()`, anything behind
        // `any`), the checker retypes this one binding to `Array<Any>`
        // and the lowerer fills it by walking the iterator protocol —
        // the index reads below then read the materialized steps and
        // need no shape of their own. The decl is inserted once the
        // pattern's step budget is known: a rest element drains to
        // exhaustion, and it can only appear last.
        let gid = self.mint_desugar_id();
        let src_name = format!("__ary_src_{gid}");
        let insert_at = lets.len();
        let mut rest_seen = false;
        let mut elem_idx: usize = 0;
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                // Build `<src_name>[elem_idx]` once; nested vs leaf
                // both consume it.
                let src_ref = self.ast.add_expr(Expr::Ident(src_name.clone()));
                let idx_lit = self.ast.add_expr(Expr::Number(elem_idx as f64));
                let elem = self.ast.add_expr(Expr::Index {
                    obj: src_ref,
                    index: idx_lit,
                });
                match self.peek() {
                    Token::Ident(n) => {
                        let nn = n.clone();
                        self.pos += 1;
                        // P-PARSE.3 — `[a = 5]` per ES spec
                        // §13.15.5.3 IteratorBindingInitialization:
                        // when the iterator is done at this index
                        // (i.e. src.length <= i) the default fires.
                        // tora's array source is fixed-length, so
                        // the runtime check collapses to a plain
                        // `src.length > i` ternary.
                        let init_expr = self.maybe_parse_destr_default(
                            elem,
                            src_name.clone(),
                            elem_idx,
                            Some(&nn),
                        )?;
                        lets.push(Stmt::LetDecl {
                            mutable: false,
                            name: nn,
                            type_ann: None,
                            init: init_expr,
                            is_var: false,
                        });
                    }
                    Token::LBracket | Token::LBrace => {
                        let nested_id = self.mint_desugar_id();
                        let nested_src = format!("__nested_destr_{nested_id}");
                        // Parse the nested body first into a temp
                        // buffer so its position advances past the
                        // closing bracket; we can then check for a
                        // trailing `= DEFAULT` that applies to the
                        // whole nested pattern (per ES spec
                        // §13.15.5.3 IteratorBindingInitialization
                        // step 4d — default fires before destructure).
                        let mut nested_body_lets: Vec<Stmt> = Vec::new();
                        self.parse_destr_into(nested_src.clone(), &mut nested_body_lets)?;
                        let init_expr =
                            self.maybe_parse_destr_default(elem, src_name.clone(), elem_idx, None)?;
                        lets.push(Stmt::LetDecl {
                            mutable: false,
                            name: nested_src.clone(),
                            type_ann: None,
                            init: init_expr,
                            is_var: false,
                        });
                        lets.extend(nested_body_lets);
                    }
                    Token::DotDotDot => {
                        // P-PARSE.6 / P-PARSE.7 — rest element in
                        // array destr per ES spec §13.15.5.3 step 4i:
                        //   `[a, b, ...rest]`         leaf rest
                        //   `[a, ...[b, c]]`          nested array
                        //   `[a, ...{x, y}]`          nested object
                        // RestPattern collects remaining iterator
                        // values into a fresh Array (`src.slice(idx)`)
                        // and then either binds it to a name or
                        // recursively destructures it.
                        self.pos += 1;
                        rest_seen = true;
                        let src_ref = self.ast.add_expr(Expr::Ident(src_name.clone()));
                        let slice_call = {
                            let slice_member = self.ast.add_expr(Expr::Member {
                                obj: src_ref,
                                name: "slice".into(),
                            });
                            let from_lit = self.ast.add_expr(Expr::Number(elem_idx as f64));
                            self.ast.add_expr(Expr::Call {
                                callee: slice_member,
                                args: vec![from_lit],
                            })
                        };
                        match self.peek() {
                            Token::Ident(n) => {
                                let nn = n.clone();
                                self.pos += 1;
                                lets.push(Stmt::LetDecl {
                                    mutable: false,
                                    name: nn,
                                    type_ann: None,
                                    init: slice_call,
                                    is_var: false,
                                });
                            }
                            Token::LBracket | Token::LBrace => {
                                // Rest target is itself a pattern —
                                // recurse with the slice as the new
                                // source. P-PARSE.7.
                                let rest_id = self.mint_desugar_id();
                                let rest_src = format!("__rest_destr_{rest_id}");
                                let mut rest_body_lets: Vec<Stmt> = Vec::new();
                                self.parse_destr_into(rest_src.clone(), &mut rest_body_lets)?;
                                lets.push(Stmt::LetDecl {
                                    mutable: false,
                                    name: rest_src,
                                    type_ann: None,
                                    init: slice_call,
                                    is_var: false,
                                });
                                lets.extend(rest_body_lets);
                            }
                            t => {
                                return Err(format!(
                                    "expected identifier or pattern after `...` in array param destructuring, got {t:?} at {}",
                                    self.at()
                                ));
                            }
                        }
                        // Rest must be last; expect closing `]`.
                        match self.peek() {
                            Token::RBracket => {}
                            t => {
                                return Err(format!(
                                    "rest element must be last in array destr, got {t:?} at {}",
                                    self.at()
                                ));
                            }
                        }
                        // Don't advance elem_idx (we'll break out on RBracket).
                    }
                    Token::Comma => {
                        // P-PARSE.6 — elision in array destructuring
                        // pattern: `[a, , c]` skips index 1 (binds
                        // nothing for that slot). Just bump elem_idx
                        // so the next slot reads from the right
                        // index.
                    }
                    t => {
                        return Err(format!(
                            "expected identifier in array param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                elem_idx += 1;
                match self.peek() {
                    Token::Comma => {
                        self.pos += 1;
                        if matches!(self.peek(), Token::RBracket) {
                            break;
                        }
                    }
                    Token::RBracket => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `]` in array param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        self.pos += 1; // consume `]`
        let group_src = self.ast.add_expr(Expr::Ident(outer_src));
        let limit: i64 = if rest_seen { -1 } else { elem_idx as i64 };
        self.ast.ary_destr_groups.insert(group_src, limit);
        lets.insert(
            insert_at,
            Stmt::LetDecl {
                mutable: false,
                name: src_name,
                type_ann: None,
                init: group_src,
                is_var: false,
            },
        );
        Ok(())
    }
}
