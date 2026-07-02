//! Destructuring-binding helpers for let/const/param positions (chunk 410).
//!
//! Extracted verbatim from parser.rs — 6 methods that lower TS binding
//! patterns (`[a, b, c]`, `{ x, y }`, `{ x: foo }`, defaults `= expr`)
//! into a synthetic-name + per-element / per-field let synthesis:
//! - parse_destr_param — entry point for fn-param destructuring
//! - parse_destr_into — dispatch on `[` vs `{` and delegate
//! - parse_destr_array_into — array-pattern element walker
//! - maybe_parse_destr_default — `= expr` default handler (array shape)
//! - maybe_parse_object_destr_default — same for object-field defaults
//! - parse_destr_object_into — object-pattern field walker
//!
//! All 6 marked `pub(super)` for cross-module impl-block access. Called
//! only from parser.rs main file (parse_fn / parse_class_member /
//! parse_let / parse_const paths). Body unchanged.

use super::*;

impl<'a> Parser<'a> {
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
        src_name: String,
        lets: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // assumes current token is `[`
        self.pos += 1;
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
                        let init_expr =
                            self.maybe_parse_destr_default(elem, src_name.clone(), elem_idx)?;
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
                            self.maybe_parse_destr_default(elem, src_name.clone(), elem_idx)?;
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
        Ok(())
    }

    /// P-PARSE.3 — peek for a `=` after a destr slot binding and
    /// wrap the load expression in a length-check ternary that
    /// substitutes the default when the source iterator is
    /// "exhausted" at this index (src.length <= elem_idx). The
    /// spec also fires the default when the value is `undefined`,
    /// but tora has no real undefined yet (P1) — once that lands
    /// the ternary should also test `=== undefined`.
    pub(super) fn maybe_parse_destr_default(
        &mut self,
        load_expr: ExprId,
        src_name: String,
        elem_idx: usize,
    ) -> Result<ExprId, String> {
        if !matches!(self.peek(), Token::Eq) {
            return Ok(load_expr);
        }
        self.pos += 1; // consume `=`
        let default_expr = self.parse_expr()?;
        // Build: src.length > elem_idx ? load_expr : default_expr
        let src_ref = self.ast.add_expr(Expr::Ident(src_name));
        let len_member = self.ast.add_expr(Expr::Member {
            obj: src_ref,
            name: "length".into(),
        });
        let idx_lit = self.ast.add_expr(Expr::Number(elem_idx as f64));
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Gt,
            left: len_member,
            right: idx_lit,
        });
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: load_expr,
            else_branch: default_expr,
        }))
    }

    /// P-PARSE.3 — `{ x = D }` / `{ x: y = D }`. Per ES spec
    /// §13.15.5.4 KeyedDestructuringAssignmentEvaluation the
    /// default fires when the looked-up value is `undefined`.
    /// tora doesn't have real undefined yet (P1) and the
    /// existing struct field path doesn't surface `missing` as
    /// a runtime value, so the default expression is parsed (so
    /// the source actually compiles) but only fires when the
    /// field type is Nullable<T> AND the load returns null.
    /// For non-Nullable struct fields the field is always
    /// present and the default is dead code — same observable
    /// behaviour as bun in the typed case.
    pub(super) fn maybe_parse_object_destr_default(
        &mut self,
        load_expr: ExprId,
    ) -> Result<ExprId, String> {
        if !matches!(self.peek(), Token::Eq) {
            return Ok(load_expr);
        }
        self.pos += 1; // consume `=`
        let default_expr = self.parse_expr()?;
        // load_expr === null ? default_expr : load_expr
        let null_lit = self.ast.add_expr(Expr::Null);
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: load_expr,
            right: null_lit,
        });
        Ok(self.ast.add_expr(Expr::Ternary {
            cond,
            then_branch: default_expr,
            else_branch: load_expr,
        }))
    }

    pub(super) fn parse_destr_object_into(
        &mut self,
        src_name: String,
        lets: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // assumes current token is `{`
        self.pos += 1;
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let (field, field_is_kw) = match self.peek() {
                    Token::Ident(n) => (n.clone(), false),
                    t if Self::keyword_property_name(t).is_some() => {
                        (Self::keyword_property_name(t).unwrap().to_string(), true)
                    }
                    t => {
                        return Err(format!(
                            "expected identifier in object param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let src_ref = self.ast.add_expr(Expr::Ident(src_name.clone()));
                let mem = self.ast.add_expr(Expr::Member {
                    obj: src_ref,
                    name: field.clone(),
                });
                if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    match self.peek() {
                        Token::Ident(n) => {
                            let nn = n.clone();
                            self.pos += 1;
                            // P-PARSE.3 — `{ x: y = D }`.
                            let init_expr = self.maybe_parse_object_destr_default(mem)?;
                            lets.push(Stmt::LetDecl {
                                mutable: false,
                                name: nn,
                                type_ann: None,
                                init: init_expr,
                                is_var: false,
                            });
                        }
                        Token::LBracket | Token::LBrace => {
                            // P-PARSE.7 — `{ x: [a, b] = [1, 2] }`.
                            // Mirror the array-destr nested-default
                            // fix from P-PARSE.6: parse the nested
                            // body FIRST so the trailing `=` becomes
                            // visible, then wrap.
                            let nested_id = self.mint_desugar_id();
                            let nested_src = format!("__nested_destr_{nested_id}");
                            let mut nested_body_lets: Vec<Stmt> = Vec::new();
                            self.parse_destr_into(nested_src.clone(), &mut nested_body_lets)?;
                            let init_expr = self.maybe_parse_object_destr_default(mem)?;
                            lets.push(Stmt::LetDecl {
                                mutable: false,
                                name: nested_src.clone(),
                                type_ann: None,
                                init: init_expr,
                                is_var: false,
                            });
                            lets.extend(nested_body_lets);
                        }
                        t => {
                            return Err(format!(
                                "expected rename target after `:` in object param destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                } else {
                    if field_is_kw {
                        return Err(format!(
                            "destructuring field `{field}` is a reserved word; use `{{ {field}: <binding> }}` to rename at {}",
                            self.at()
                        ));
                    }
                    let init_expr = self.maybe_parse_object_destr_default(mem)?;
                    lets.push(Stmt::LetDecl {
                        mutable: false,
                        name: field,
                        type_ann: None,
                        init: init_expr,
                        is_var: false,
                    });
                }
                match self.peek() {
                    Token::Comma => {
                        self.pos += 1;
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                    }
                    Token::RBrace => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `}}` in object param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        self.pos += 1; // consume `}`
        Ok(())
    }
}
