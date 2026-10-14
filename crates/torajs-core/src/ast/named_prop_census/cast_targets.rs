//! The optional `number` members a cast from the any lane can write a
//! `null` into.
//!
//! An any-lane value entering an annotated object layout is cast field
//! by field (`torajs_anyvalue::anyv_cast`); an optional `number`
//! member's f64 slot spells a number or `undefined`, and a `null` the
//! value carries (`JSON.parse('{"a":null}')` into `{ a?: number }`)
//! goes to the field's sidecar behind `FLAG_OBJ_EXOTIC_FIELD`, as a
//! write through the any lane would put it. A typed read of such a
//! member has to answer the sidecar, so this census names the optional
//! `number` members of every layout a cast can reach: the layouts of
//! the annotated slots an `any` value is seen entering — a binding, a
//! field or element store, a return, a `reduce` result, a spread call
//! — and of the parameters of every function the any lane can call
//! (the bodies `ssa_lower_boxed_entry` gives an adapter: a lifted
//! closure, a method, a factory, a twin). The members of the layouts
//! those layouts contain count too, since the cast copies them along.
//! A layout no any-lane value is seen entering keeps its bare loads.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, PropKey, Stmt};
use crate::check::{GenericAliasMap, Type};
use crate::check_type_ann::{resolve_type_ann_decl, resolve_type_ann_field};

/// `(aliases, generic aliases)`: what an annotation resolves against.
type Resolve<'a> = (&'a HashMap<String, Type>, &'a GenericAliasMap);

struct Census<'a> {
    ast: &'a Ast,
    expr_types: &'a HashMap<ExprId, Type>,
    resolve: Resolve<'a>,
    optional_number: &'a HashSet<PropKey>,
    /// The class names already walked (a class may contain itself).
    seen: HashSet<String>,
    /// The annotations already walked: a cast into one reaches the same
    /// layouts each time.
    anns: HashSet<&'a str>,
    out: HashSet<String>,
    /// Some reached layout has an optional member of any type: a cast
    /// of a value lacking the key builds a cell without it.
    optional: bool,
}

/// What the census found: the optional `number` member names a cast
/// can write a `null` into, and whether a cast can leave any optional
/// member out of a cell.
pub(super) struct CastTargets {
    pub(super) number_names: HashSet<String>,
    pub(super) optional: bool,
}

/// The layouts a typed read converts a field's value into when the
/// field may hold an accessor's answer or a value parked in its sidecar
/// (`exotic_read::convert`, the object and array rows): the read casts
/// that value into a fresh cell of the field's declared layout, and a
/// `null` for an optional `number` member of it lands in the new cell's
/// sidecar as any cast's does. `converted(name)` says whether a read of
/// a field called `name` can take the conversion.
pub(super) fn converted_fields(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    resolve: Resolve<'_>,
    optional_number: &HashSet<PropKey>,
    converted: &dyn Fn(&str) -> bool,
) -> CastTargets {
    let mut c = Census {
        ast,
        expr_types,
        resolve,
        optional_number,
        seen: HashSet::new(),
        anns: HashSet::new(),
        out: HashSet::new(),
        optional: false,
    };
    for s in &ast.stmts {
        let Stmt::TypeDecl {
            type_params,
            fields,
            ..
        } = s
        else {
            continue;
        };
        // a generic declaration's field whose annotation names none of
        // its type parameters has the same layout in every instance
        for (k, ann) in fields {
            if k.as_wtf8().as_str().is_some_and(converted)
                && !crate::ast_throw_info::obj_cast::names_type_param(ann, type_params)
            {
                c.ann(ann);
            }
        }
    }
    CastTargets {
        number_names: c.out,
        optional: c.optional,
    }
}

/// The layouts a cast can reach, summed up ([`CastTargets`]).
pub(super) fn names(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    resolve: Resolve<'_>,
    optional_number: &HashSet<PropKey>,
) -> CastTargets {
    let mut c = Census {
        ast,
        expr_types,
        resolve,
        optional_number,
        seen: HashSet::new(),
        anns: HashSet::new(),
        out: HashSet::new(),
        optional: false,
    };
    for s in &ast.stmts {
        c.stmt(s, None);
    }
    for (i, e) in ast.exprs.iter().enumerate() {
        c.expr(ExprId(i as u32), e);
    }
    CastTargets {
        number_names: c.out,
        optional: c.optional,
    }
}

impl<'a> Census<'a> {
    fn any(&self, e: ExprId) -> bool {
        matches!(self.expr_types.get(&e), Some(Type::Any))
    }

    /// The statements of one function body (`ret` its return
    /// annotation): a declaration's annotation an `any` init enters, a
    /// return of an `any` value, and the functions declared inside.
    fn stmt(&mut self, s: &'a Stmt, ret: Option<&'a str>) {
        match s {
            Stmt::LetDecl {
                type_ann: Some(ann),
                init,
                ..
            }
            | Stmt::UsingDecl {
                type_ann: Some(ann),
                init,
                ..
            }
            | Stmt::YieldInto {
                type_ann: Some(ann),
                value: init,
                ..
            } if self.any(*init) => self.ann(ann),
            Stmt::ForOf {
                var_type_ann: Some(ann),
                elem_expr,
                body,
                ..
            } => {
                if self.any(*elem_expr) {
                    self.ann(ann);
                }
                self.stmt(body, ret);
            }
            Stmt::Return(Some(e)) if self.any(*e) => {
                if let Some(ann) = ret {
                    self.ann(ann);
                }
            }
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                if any_lane_callable(name, params) {
                    for p in params {
                        if let Some(ann) = &p.type_ann
                            && !matches!(p.name.as_str(), "__env" | "__this")
                        {
                            self.ann(ann);
                        }
                    }
                }
                for s in body {
                    self.stmt(s, return_type.as_deref());
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.stmt(then_branch, ret);
                if let Some(e) = else_branch {
                    self.stmt(e, ret);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => self.stmt(body, ret),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    self.stmt(i, ret);
                }
                self.stmt(body, ret);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    for s in &c.body {
                        self.stmt(s, ret);
                    }
                }
                for s in default.iter().flatten() {
                    self.stmt(s, ret);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for s in body
                    .iter()
                    .chain(catch_body)
                    .chain(finally_body.iter().flatten())
                {
                    self.stmt(s, ret);
                }
            }
            Stmt::Block(ss) | Stmt::Multi(ss) => {
                for s in ss {
                    self.stmt(s, ret);
                }
            }
            Stmt::ExportDecl { inner: Some(s), .. } => self.stmt(s, ret),
            _ => {}
        }
    }

    /// One expression: an `any` value stored into a typed slot, a
    /// `reduce` whose result is typed, a call spreading an `any` array
    /// over typed parameters.
    fn expr(&mut self, eid: ExprId, e: &Expr) {
        match e {
            Expr::Assign { target, value } if self.any(*value) => {
                if let Some(t) = self.slot_type(*target) {
                    self.ty(&t);
                }
            }
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee) {
                    match name.as_str() {
                        "reduce" | "reduceRight" => {
                            if let Some(t) = self.expr_types.get(&eid).cloned() {
                                self.ty(&t);
                            }
                        }
                        "push" | "unshift" if args.iter().any(|a| self.any(*a)) => {
                            if let Some(Type::Array(t)) = self.expr_types.get(obj) {
                                self.ty(&t.clone());
                            }
                        }
                        _ => {}
                    }
                }
                let spread_any = args.iter().any(|a| match self.ast.get_expr(*a) {
                    Expr::Spread { expr } => match self.expr_types.get(expr) {
                        Some(Type::Any) => true,
                        Some(Type::Array(t)) => **t == Type::Any,
                        _ => false,
                    },
                    _ => false,
                });
                if spread_any && let Some(Type::Function(ps, _)) = self.expr_types.get(callee) {
                    for p in ps.clone() {
                        self.ty(&p);
                    }
                }
            }
            _ => {}
        }
    }

    /// The type of the slot a store to `target` fills.
    fn slot_type(&self, target: ExprId) -> Option<Type> {
        match self.ast.get_expr(target) {
            Expr::Ident(_) => self.expr_types.get(&target).cloned(),
            Expr::Index { obj, .. } => match self.expr_types.get(obj) {
                Some(Type::Array(t)) => Some((**t).clone()),
                _ => None,
            },
            Expr::Member { obj, name } => {
                let mut t = self.expr_types.get(obj);
                while let Some(Type::Nullable(inner)) = t {
                    t = Some(inner);
                }
                match t {
                    Some(Type::Struct(fs)) => fs
                        .iter()
                        .find(|(k, _)| *k == name.as_str())
                        .map(|(_, t)| t.clone()),
                    Some(Type::ClassRef(_)) => {
                        let ann = crate::ast_throw_info::class_field_ann(
                            self.ast,
                            self.expr_types,
                            *obj,
                            name,
                        )?;
                        self.resolve_ann(&ann)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn resolve_ann(&self, ann: &str) -> Option<Type> {
        resolve_type_ann_decl(ann, self.resolve.0, &[], self.resolve.1)
    }

    fn ann(&mut self, ann: &'a str) {
        if holds_no_layout(ann) || !self.anns.insert(ann) {
            return;
        }
        if let Some(t) = self.resolve_ann(ann) {
            self.ty(&t);
        }
    }

    /// The optional `number` members of the layouts a value of type `t`
    /// is cast into, this one and the ones its fields hold.
    fn ty(&mut self, t: &Type) {
        match t {
            Type::Struct(fs) => {
                self.optional |= !fs.optional().is_empty();
                for (k, ft) in fs.iter() {
                    if fs.is_optional(k)
                        && matches!(ft, Type::Nullable(n) if **n == Type::Number)
                        && let Some(k) = k.as_wtf8().as_str()
                    {
                        self.out.insert(k.to_string());
                    }
                    self.ty(ft);
                }
            }
            Type::ClassRef(name) => self.class(name),
            Type::Array(t) | Type::Nullable(t) => self.ty(t),
            Type::Union(ts) => {
                for t in ts {
                    self.ty(t);
                }
            }
            _ => {}
        }
    }

    /// The declared fields of the class `name`: its optional `number`
    /// members, and the layouts of its other fields.
    fn class(&mut self, name: &str) {
        if !self.seen.insert(name.to_string()) {
            return;
        }
        // an instance of a generic class reads its declaration's
        // fields, the ones every instance lays out alike
        let Some((decl, fields)) = crate::ast_throw_info::obj_cast::decl_fields(self.ast, name)
        else {
            return;
        };
        let name = decl;
        for (k, ann) in fields {
            self.optional |= self.ast.optional_members.declared(name, &k);
            let plain_optional = self.ast.optional_members.plain_at(name, &k);
            if plain_optional
                && ann == crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN
                && self.optional_number.contains(&k)
                && let Some(k) = k.as_wtf8().as_str()
            {
                self.out.insert(k.to_string());
                continue;
            }
            if let Some(t) = resolve_type_ann_field(&ann, self.resolve.0, &[], self.resolve.1) {
                self.ty(&t);
            }
        }
    }
}

/// An annotation naming a primitive, or an array of one: no layout.
fn holds_no_layout(ann: &str) -> bool {
    matches!(
        ann.trim().trim_end_matches("[]"),
        "number"
            | "string"
            | "boolean"
            | "bigint"
            | "symbol"
            | "any"
            | "unknown"
            | "never"
            | "void"
            | "undefined"
            | "null"
    )
}

/// Whether the any lane can call this function: the shapes
/// `ssa_lower_boxed_entry` builds an adapter for — a lifted closure
/// body (`__env` first), a method body (`__this` first), a static
/// method, a class factory, a receiver-polymorphic twin.
fn any_lane_callable(name: &str, params: &[Param]) -> bool {
    params
        .first()
        .is_some_and(|p| matches!(p.name.as_str(), "__env" | "__this"))
        || name.starts_with("__sm_")
        || name.starts_with("__new_")
        || crate::ast::is_generator_any_twin_decl(name)
}
