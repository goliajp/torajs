//! What the census judges every candidate against: the program's
//! declarations and roads, read once ([`Program`]), and one
//! candidate's reading of the checker types ([`Holder`]).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, PropKey, Stmt};
use crate::check::Type;

use super::super::arr_head_census::ExprMask;
use super::super::live_field_census::walk::nested_stmts;
use super::Candidate;

/// Identifiers that reach a value by a road no typed census follows.
const ROADS: &[&str] = &[
    "Reflect",
    "eval",
    "Function",
    "Proxy",
    "globalThis",
    "arguments",
];

pub(super) struct FnInfo<'a> {
    pub params: &'a [Param],
    pub body: &'a [Stmt],
    pub ret: Option<&'a str>,
    pub generic: bool,
    /// How many declarations carry the name.
    pub decls: usize,
}

/// A type declaration: its name, whether it is a class, its fields.
pub(super) type TypeDecl<'a> = (&'a str, bool, &'a [(PropKey, String)]);

pub(super) struct Program<'a> {
    pub ast: &'a Ast,
    pub types: &'a HashMap<ExprId, Type>,
    pub dead: &'a ExprMask,
    pub fns: HashMap<&'a str, FnInfo<'a>>,
    /// Names some `let`, parameter, loop variable or catch binds.
    pub bound: HashSet<&'a str>,
    pub type_decls: Vec<TypeDecl<'a>>,
    pub road: Option<&'a str>,
    /// Some property of some class is an accessor: a member read can
    /// run user code.
    pub accessors: bool,
}

impl<'a> Program<'a> {
    pub(super) fn index(
        ast: &'a Ast,
        types: &'a HashMap<ExprId, Type>,
        dead: &'a ExprMask,
    ) -> Self {
        let mut p = Program {
            ast,
            types,
            dead,
            fns: HashMap::new(),
            bound: HashSet::new(),
            type_decls: Vec::new(),
            road: None,
            accessors: !ast.accessor_getters.is_empty() || !ast.accessor_setters.is_empty(),
        };
        p.stmts(&ast.stmts);
        for (i, e) in ast.exprs.iter().enumerate() {
            if dead.contains(ExprId(i as u32)) {
                continue;
            }
            match e {
                Expr::Ident(n) if ROADS.contains(&n.as_str()) => p.road = Some(n),
                Expr::NewDynamic { .. } => p.road = Some("a dynamic `new`"),
                Expr::ArrowFn { params, body, .. } => {
                    p.bound.extend(params.iter().map(|q| q.name.as_str()));
                    p.stmts(body);
                }
                _ => {}
            }
        }
        p
    }

    fn stmts(&mut self, list: &'a [Stmt]) {
        for s in list {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::FnDecl {
                name,
                params,
                body,
                return_type,
                type_params,
                ..
            } => {
                let info = self.fns.entry(name).or_insert(FnInfo {
                    params,
                    body,
                    ret: return_type.as_deref(),
                    generic: !type_params.is_empty(),
                    decls: 0,
                });
                info.decls += 1;
                self.bound.extend(params.iter().map(|q| q.name.as_str()));
                self.stmts(body);
            }
            Stmt::TypeDecl { name, fields, .. } => {
                let class = self.ast.class_parents.contains_key(name);
                self.type_decls.push((name, class, fields));
            }
            Stmt::LetDecl { name, .. }
            | Stmt::UsingDecl { name, .. }
            | Stmt::YieldInto { var: name, .. }
            | Stmt::ForOf { var_name: name, .. }
            | Stmt::ForOfSplitIter { var_name: name, .. } => {
                self.bound.insert(name);
            }
            Stmt::Try {
                catch_param: Some(c),
                ..
            } => {
                self.bound.insert(c);
            }
            _ => {}
        }
        nested_stmts(s, &mut |st| self.stmt(st));
    }

    pub(super) fn ty(&self, e: ExprId) -> Option<&'a Type> {
        self.types.get(&e)
    }
}

/// What a checker type holds of the candidate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Cat {
    /// An instance.
    Elem,
    /// An array of instances.
    Arr,
    /// An instance of the container class with this index.
    Boxed(usize),
    /// Anything else that may reach an instance.
    Bad,
}

pub(super) struct Holder<'a> {
    pub class: &'a str,
    pub fields: &'a [&'a str],
    pub arity: usize,
    /// Classes a field of which holds an instance, directly or through
    /// another container, with their field names.
    pub containers: Vec<(&'a str, Vec<&'a str>)>,
    /// Per container, its field annotations in the order of `containers`.
    anns: Vec<Vec<&'a str>>,
    /// Container fields whose type holds an instance.
    pub holding_fields: HashSet<&'a str>,
    /// Every declared name whose declaration mentions the class.
    mention: HashSet<&'a str>,
}

impl<'a> Holder<'a> {
    pub(super) fn new(prog: &Program<'a>, c: &'a Candidate<'a>) -> Result<Self, String> {
        let mut mention: HashSet<&str> = HashSet::from([c.name]);
        loop {
            let before = mention.len();
            for (name, _, fields) in &prog.type_decls {
                if !mention.contains(name) && fields.iter().any(|(_, a)| words_hit(a, &mention)) {
                    mention.insert(name);
                }
            }
            if mention.len() == before {
                break;
            }
        }
        let ast = prog.ast;
        let mut containers = Vec::new();
        let mut anns = Vec::new();
        let mut holding_fields = HashSet::new();
        for (name, class, fields) in &prog.type_decls {
            if *name == c.name || !mention.contains(name) {
                continue;
            }
            if !class {
                if matches!(fields, [(k, _)] if k.lossy() == "__alias__") {
                    continue;
                }
                return Err(format!("the struct type `{name}` holds it"));
            }
            if !matches!(ast.class_parents.get(*name), Some(None))
                || ast
                    .class_parents
                    .values()
                    .any(|p| p.as_deref() == Some(*name))
                || ast
                    .class_type_params
                    .get(*name)
                    .is_some_and(|t| !t.is_empty())
                || ast.private_elements.contains_key(*name)
                || ast.ctor_return_override.contains(*name)
                || ast.accessor_getters.keys().any(|(o, _)| o == name)
                || ast.accessor_setters.keys().any(|(o, _)| o == name)
            {
                return Err(format!("the container `{name}` is not a plain root class"));
            }
            let mut names = Vec::new();
            anns.push(fields.iter().map(|(_, a)| a.as_str()).collect());
            for (k, a) in fields.iter() {
                let std::borrow::Cow::Borrowed(f) = k.lossy() else {
                    return Err(format!("the container `{name}` has an ill-formed key"));
                };
                if words_hit(a, &mention) {
                    holding_fields.insert(f);
                }
                names.push(f);
            }
            containers.push((*name, names));
        }
        Ok(Holder {
            class: c.name,
            fields: &c.fields,
            arity: c.arity,
            containers,
            anns,
            holding_fields,
            mention,
        })
    }

    /// What field `name` of container `i` is declared to hold.
    pub(super) fn field_cat(&self, i: usize, name: &str) -> Option<Cat> {
        let at = self.containers[i].1.iter().position(|f| *f == name)?;
        self.ann_cat(self.anns[i][at])
    }

    pub(super) fn classify(&self, t: &Type) -> Option<Cat> {
        let inner_bad = |t: &Type| self.classify(t).map(|_| Cat::Bad);
        match t {
            Type::ClassRef(n) if n == self.class => Some(Cat::Elem),
            Type::ClassRef(n) => match self.containers.iter().position(|(c, _)| c == n) {
                Some(i) => Some(Cat::Boxed(i)),
                None => words_hit(n, &self.mention).then_some(Cat::Bad),
            },
            Type::Array(inner) => match inner.as_ref() {
                Type::ClassRef(n) if n == self.class => Some(Cat::Arr),
                other => inner_bad(other),
            },
            // a struct of exactly the class's field names may share its
            // layout; a wider one reaches an instance slot only through
            // a place the walk sees typed as the slot
            Type::Struct(fields) => {
                let same = fields.len() == self.fields.len()
                    && (self.fields.iter()).all(|f| fields.iter().any(|(k, _)| k.lossy() == *f));
                (same || fields.iter().any(|(_, t)| self.classify(t).is_some())).then_some(Cat::Bad)
            }
            Type::Nullable(x) | Type::Promise(x) | Type::MapIter(x) | Type::Rest(x) => inner_bad(x),
            Type::Map(a, b) | Type::Dict(a, b) => inner_bad(a).or_else(|| inner_bad(b)),
            Type::Union(v) => v.iter().find_map(inner_bad),
            Type::Function(ps, r) => ps.iter().find_map(inner_bad).or_else(|| inner_bad(r)),
            _ => None,
        }
    }

    pub(super) fn cat_of(&self, prog: &Program<'_>, e: ExprId) -> Option<Cat> {
        self.classify(prog.ty(e)?)
    }

    /// What a declared annotation holds, read exactly: `C`, `C[]` /
    /// `Array<C>`, a container's name; any other spelling that mentions
    /// one of them is [`Cat::Bad`].
    pub(super) fn ann_cat(&self, ann: &str) -> Option<Cat> {
        let ann = ann.trim();
        let c = self.class;
        if ann == c {
            return Some(Cat::Elem);
        }
        if ann.strip_suffix("[]") == Some(c)
            || ann.strip_prefix("Array<").and_then(|a| a.strip_suffix('>')) == Some(c)
        {
            return Some(Cat::Arr);
        }
        if let Some(i) = self.containers.iter().position(|(n, _)| *n == ann) {
            return Some(Cat::Boxed(i));
        }
        words_hit(ann, &self.mention).then_some(Cat::Bad)
    }
}

/// Some identifier-shaped word of a type spelling is in `names`.
fn words_hit(spelling: &str, names: &HashSet<&str>) -> bool {
    spelling
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
        .any(|w| !w.is_empty() && names.contains(w))
}
