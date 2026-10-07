//! Which functions answer a property read or write as an accessor.
//!
//! The class-accessor walks up the parent chain live in [`chain`];
//! this file answers the by-name questions (object-literal getters and
//! the program-wide getter union) that the sentinel tables ask.

mod chain;
pub(crate) use chain::{
    accessor_getter_in_chain, accessor_setter_in_chain, declared_setter_in_chain, getter_may_run,
};

use std::collections::HashMap;

use super::Ast;

pub(crate) struct AccessorHit {
    pub fn_name: String,
    /// The declaring class's type-param names zipped against the
    /// instantiation-argument spellings composed down the walk —
    /// empty for a non-generic declarer.
    pub subst: Vec<(String, String)>,
}

/// The lifted function names of OBJECT-LITERAL getters for `prop`.
///
/// A class accessor is recorded in `ast.accessor_getters` under its
/// declaring class, which is what the [`chain`] walks resolve. An object
/// literal has no class to record under: the parser rewrites `get x()
/// {…}` into a `__getter_x` field holding an arrow, and the lift
/// turns that arrow into a `Closure { fn_name }`. So the only place
/// the connection survives is the literal itself, and the only way to
/// ask is by property name.
///
/// By name and across every literal in the program, like the sentinel
/// tables that call this: two literals with a `g` getter answer both
/// names, and a caller that is asking "can a read of `.g` hand back
/// the sentinel" wants exactly that union.
pub(crate) fn objlit_getter_fns<'a>(ast: &'a Ast, prop: &str) -> Vec<&'a str> {
    let slot = torajs_wtf8::Wtf8Buf::from(format!("__getter_{prop}"));
    let mut out = Vec::new();
    for id in 0..ast.exprs.len() {
        let super::Expr::ObjectLit { fields } = ast.get_expr(super::ExprId(id as u32)) else {
            continue;
        };
        for (fname, value) in fields {
            if fname.as_wtf8() != slot.as_ref() {
                continue;
            }
            if let super::Expr::Closure { fn_name, .. } = ast.get_expr(*value) {
                out.push(fn_name.as_str());
            }
        }
    }
    out
}

/// Every function a read of property `prop` can run as a getter:
/// instance accessors, static accessors, and object-literal getters.
///
/// The three live in three tables keyed three different ways — the
/// first two by DECLARING CLASS, the third only by the literal's own
/// `__getter_<prop>` field — so asking "which function does reading
/// `.g` run" means asking all three and taking the union. By name
/// and program-wide, which is the same trade the sentinel tables
/// already make: two classes with a `g` accessor answer both, and a
/// caller weighing "can this read hand back the sentinel" wants the
/// union, because missing one is silent and including one costs a
/// predictable compare.
///
/// The caller supplies its own "and can that function hand one back"
/// test, because the two callers ask it of different objects at
/// different times: the width analysis asks the sentinel table it is
/// still closing, lowering asks the finished width table. The
/// name -> function hop is the half that must not be spelled twice.
pub(crate) fn getter_fn_names<'a>(ast: &'a Ast, prop: &str) -> Vec<&'a str> {
    let key = torajs_wtf8::Wtf8::new(prop);
    let mut out: Vec<&'a str> = ast
        .accessor_getters
        .iter()
        .chain(ast.static_accessor_getters.iter())
        .filter(|((_, p), _)| p.as_wtf8() == key)
        .map(|(_, f)| f.as_str())
        .collect();
    out.extend(objlit_getter_fns(ast, prop));
    out
}

/// Every getter by the property name it serves, in one arena pass:
/// [`getter_fn_names`] for all names at once.
pub(crate) fn getter_fns_by_prop(ast: &Ast) -> HashMap<String, Vec<&str>> {
    let mut out: HashMap<String, Vec<&str>> = HashMap::new();
    let class = ast.accessor_getters.iter();
    for ((_, p), f) in class.chain(ast.static_accessor_getters.iter()) {
        let prop = p.as_wtf8().to_string_lossy().into_owned();
        out.entry(prop).or_default().push(f.as_str());
    }
    for id in 0..ast.exprs.len() {
        let super::Expr::ObjectLit { fields } = ast.get_expr(super::ExprId(id as u32)) else {
            continue;
        };
        for (fname, value) in fields {
            let name = fname.as_wtf8().to_string_lossy();
            if let Some(prop) = name.strip_prefix("__getter_")
                && let super::Expr::Closure { fn_name, .. } = ast.get_expr(*value)
            {
                out.entry(prop.to_string())
                    .or_default()
                    .push(fn_name.as_str());
            }
        }
    }
    out
}
