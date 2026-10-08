//! The pieces of an enum declaration that become other declarations:
//! the object literal holding the members and reverse mappings, the
//! type alias the enum's name annotates with, and the rewrite of a
//! computed initializer's member references.

use super::super::*;
use super::const_eval::{EnumConst, EnumValue, is_array_index, member_const};
use crate::ast::{PropKey, number_prop_key};
use torajs_wtf8::Wtf8Buf;

impl Parser<'_> {
    /// A bare member name inside a computed initializer refers to the
    /// enum's own member (TS §enum member scope), and so do `E.M` and
    /// `E["M"]`; the hoisted initializer runs before the object
    /// exists, so spell each one out.
    pub(super) fn resolve_member_refs(
        &mut self,
        root: ExprId,
        members: &[(PropKey, EnumValue)],
        enum_name: &str,
    ) {
        let mut stack = vec![root];
        while let Some(eid) = stack.pop() {
            let own =
                |obj: &ExprId| matches!(self.ast.get_expr(*obj), Expr::Ident(o) if o == enum_name);
            let replacement = match self.ast.get_expr(eid) {
                Expr::Ident(x) => member_const(members, x).cloned(),
                Expr::Member { obj, name } if own(obj) => member_const(members, name).cloned(),
                Expr::Index { obj, index } if own(obj) => match self.ast.get_expr(*index) {
                    Expr::String(s) => s.as_str().and_then(|k| member_const(members, k)).cloned(),
                    _ => None,
                },
                _ => None,
            };
            match replacement {
                Some(EnumValue::Const(EnumConst::Num(n))) => {
                    self.ast.exprs[eid.0 as usize] = Expr::Number(n)
                }
                Some(EnumValue::Const(EnumConst::Str(s))) => {
                    self.ast.exprs[eid.0 as usize] = Expr::String(s)
                }
                Some(EnumValue::Computed(tmp)) => self.ast.exprs[eid.0 as usize] = Expr::Ident(tmp),
                None => stack.extend(crate::ast::desugar_with::walk::expr_children(
                    &self.ast, eid,
                )),
            }
        }
    }

    /// Property order follows §10.1.11.1 OrdinaryOwnPropertyKeys:
    /// array-index reverse entries ascending, then the members and
    /// their other reverse entries in declaration order.
    pub(super) fn enum_object_fields(
        &mut self,
        members: &[(PropKey, EnumValue)],
    ) -> Vec<(PropKey, ExprId)> {
        let mut index_entries: std::collections::BTreeMap<u32, PropKey> =
            std::collections::BTreeMap::new();
        for (k, v) in members {
            if let EnumValue::Const(EnumConst::Num(n)) = v
                && is_array_index(*n)
            {
                index_entries.insert(*n as u32, k.clone());
            }
        }
        let mut fields: Vec<(PropKey, ExprId)> = Vec::with_capacity(members.len() * 2);
        for (idx, k) in index_entries {
            let rev = self.ast.add_expr(Expr::String(Wtf8Buf::from(k.as_ref())));
            fields.push((number_prop_key(f64::from(idx)), rev));
        }
        for (k, v) in members {
            match v {
                EnumValue::Const(EnumConst::Num(n)) => {
                    let val = self.ast.add_expr(Expr::Number(*n));
                    fields.push((k.clone(), val));
                    if !is_array_index(*n) {
                        let rev = self.ast.add_expr(Expr::String(Wtf8Buf::from(k.as_ref())));
                        fields.push((number_prop_key(*n), rev));
                    }
                }
                EnumValue::Const(EnumConst::Str(s)) => {
                    let val = self.ast.add_expr(Expr::String(s.clone()));
                    fields.push((k.clone(), val));
                }
                EnumValue::Computed(tmp) => {
                    let val = self.ast.add_expr(Expr::Ident(tmp.clone()));
                    fields.push((k.clone(), val));
                    let key = self.ast.add_expr(Expr::Ident(tmp.clone()));
                    let rev = self.ast.add_expr(Expr::String(Wtf8Buf::from(k.as_ref())));
                    let sentinel = PropKey::from(format!(
                        "__computed_{}__",
                        self.ast.objlit_computed_keys.len()
                    ));
                    self.ast.objlit_computed_keys.insert(rev, key);
                    fields.push((sentinel, rev));
                }
            }
        }
        fields
    }

    /// The alias the enum's name annotates with, registered under the
    /// name itself at the top level and under a name of its own in a
    /// nested scope, so two local enums of one name keep their own
    /// types.
    pub(super) fn register_enum_type(
        &mut self,
        name: &str,
        members: &[(PropKey, EnumValue)],
    ) -> String {
        let alias = if self.stmt_depth == 1 || self.enum_exported() {
            name.to_string()
        } else {
            format!("__enum_{name}_{}", self.mint_desugar_id())
        };
        let decl = Stmt::TypeDecl {
            name: alias.clone(),
            type_params: Vec::new(),
            fields: vec![(PropKey::from("__alias__"), enum_type(members).to_string())],
        };
        // An exported enum exports its type too: a named import of
        // the value injects the alias only through an export face.
        let decl = if self.enum_exported() {
            Stmt::ExportDecl {
                inner: Some(Box::new(decl)),
                named: Vec::new(),
                default_expr: None,
                source: None,
                star: None,
                type_only: false,
                type_named: Vec::new(),
            }
        } else {
            decl
        };
        self.synth_classes.push(decl);
        alias
    }

    /// A merged declaration may change the enum's type (string
    /// members joining numeric ones): rewrite the registered alias.
    pub(super) fn update_enum_alias(&mut self, state: usize) {
        let ty = enum_type(&self.enums.states[state].members).to_string();
        let alias = &self.enums.states[state].alias;
        let decls = self
            .synth_classes
            .iter_mut()
            .chain(self.ast.stmts.iter_mut());
        for s in decls {
            let s = match s {
                Stmt::ExportDecl {
                    inner: Some(inner), ..
                } => inner.as_mut(),
                s => s,
            };
            if let Stmt::TypeDecl { name, fields, .. } = s
                && name == alias
            {
                fields[0].1 = ty;
                return;
            }
        }
    }
}

/// A numeric enum's type is `number` (a computed member is a number
/// too), a string enum's is `string`. A heterogeneous enum's type is
/// `number | string`; the annotation grammar has no such union, so its
/// values are typed at runtime (`any`).
fn enum_type(members: &[(PropKey, EnumValue)]) -> &'static str {
    let strings = members
        .iter()
        .filter(|(_, v)| matches!(v, EnumValue::Const(EnumConst::Str(_))))
        .count();
    if strings == 0 {
        "number"
    } else if strings == members.len() {
        "string"
    } else {
        "any"
    }
}
