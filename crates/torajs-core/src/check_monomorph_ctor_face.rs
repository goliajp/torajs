//! RFC 20260917-plain-fn-new-target knife 2 — the construct face of a
//! function expression that reads `new.target`.
//!
//! `ast/new_target_param.rs` turned the expression's reads into a
//! body-level `let __new_target = undefined`, which is right for every
//! [[Call]] through the closure value. [[Construct]] needs the body
//! with the real new.target, so this step clones the LIFTED body into
//! `<closure>$$ntctor(__env, __this, __new_target, …user)`, the local
//! dropped in favour of the parameter.
//!
//! The clone is minted here, after the main check, for the reason the
//! 551-03 any-widen clones are: it has no construction site of its own.
//! The construct kernel enters it with the ORIGINAL cell as env
//! (`widen_clone_origin` gives the lowering the original's capture
//! layout), and it is checked with the original's recorded capture
//! types (`check_closure_body`) — the scope a nested closure's captures
//! resolved in is long gone by now. The closure cell mint registers the
//! face against the original's boxed entry (`ssa_lower_closure_ctor_face.rs`).
//!
//! The face's boxed adapter skips both head slots when it counts the
//! arguments (`ssa_lower_boxed_entry` `recv_slots`), so a parameter
//! default and a rest parameter read the same argument window the call
//! channel does. A body with an arguments vector keeps the call channel
//! (the undefined local).

use std::collections::{HashMap, VecDeque};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Checker;
use crate::check_monomorph::WorkItem;
use crate::ssa_lower_generics_monomorph::Generics;

/// The construct face's name for lifted closure `fn_name`.
pub(crate) fn face_name(fn_name: &str) -> String {
    format!("{fn_name}$$ntctor")
}

fn hidden(name: &str) -> Param {
    Param {
        name: name.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    }
}

/// Mint the faces not minted yet; answers whether it minted any — a
/// specialization cloned after the first call brings new sites.
pub(crate) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
) -> bool {
    let mut minted = false;
    let mut sites: Vec<_> = owned_ast.new_target_fn_exprs.iter().copied().collect();
    sites.sort_by_key(|e| e.0);
    for site in sites {
        let Expr::Closure { fn_name, .. } = owned_ast.get_expr(site) else {
            continue;
        };
        let fn_name = fn_name.clone();
        let face = face_name(&fn_name);
        if c.closure_fn_names.contains(&face) {
            continue;
        }
        let Some((params, return_type, body)) = owned_ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                ..
            } if *name == fn_name => Some((params.clone(), return_type.clone(), body.clone())),
            _ => None,
        }) else {
            continue;
        };
        if params.first().is_none_or(|p| p.name != "__env")
            || params.iter().any(|p| p.name == "__torajs_argv")
        {
            continue;
        }
        let Some(cap_tys) = c.closure_captures.get(&fn_name).cloned() else {
            continue;
        };
        let (mut new_params, new_return_type, mut new_body, _) =
            crate::check_monomorph::clone_spec_body(owned_ast, &params, &return_type, &body, &[]);
        let Some(local) = new_body.iter().position(|s| {
            matches!(s, Stmt::LetDecl { name, .. } if name == crate::ast::new_target_param::NEW_TARGET)
        }) else {
            continue;
        };
        new_body.remove(local);
        if new_params.get(1).is_none_or(|p| p.name != "__this") {
            new_params.insert(1, hidden("__this"));
        }
        new_params.insert(2, hidden(crate::ast::new_target_param::NEW_TARGET));
        c.closure_fn_names.insert(face.clone());
        crate::check_monomorph_closures::mirror_name_keyed_tables(owned_ast, &fn_name, &face);
        c.widen_clone_origin.insert(face.clone(), fn_name.clone());
        owned_ast.stmts.push(Stmt::FnDecl {
            name: face.clone(),
            type_params: Vec::new(),
            params: new_params,
            return_type: new_return_type,
            body: new_body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });
        let saved_errors = c.errors.len();
        let saved_sites = std::mem::take(&mut c.generic_call_sites);
        let saved_pads = std::mem::take(&mut c.arity_pad_count);
        // checked again while that grows the widen set, as a
        // specialization is (`let_widen::check_spec`)
        let face_decl = owned_ast
            .stmts
            .last()
            .cloned()
            .expect("the face was just pushed");
        let mut checked = Ok(crate::check::Type::Any);
        let check = |c: &mut Checker, a: &Ast| {
            let caps = cap_tys.clone();
            checked = crate::check_type_of_fn::check_closure_body(c, a, site, &face, caps);
        };
        crate::let_widen::check_spec(c, owned_ast, &face_decl, check);
        let inner_sites = std::mem::replace(&mut c.generic_call_sites, saved_sites);
        let inner_pads = std::mem::replace(&mut c.arity_pad_count, saved_pads);
        // The clone's calls to generic functions are fresh ExprIds: seed
        // them like the original's were, or they call the template.
        let mut inner: Vec<(ExprId, (String, Vec<crate::check::Type>))> =
            inner_sites.iter().map(|(k, v)| (*k, v.clone())).collect();
        inner.sort_by_key(|(eid, _)| eid.0);
        let decl = owned_ast
            .stmts
            .iter()
            .rev()
            .find(|s| matches!(s, Stmt::FnDecl { name, .. } if *name == face))
            .cloned();
        if let Some(decl) = &decl {
            let param_env = crate::check_monomorph::spec_params_env(decl);
            crate::check_monomorph::seed_records(
                owned_ast,
                &inner,
                generics,
                &param_env,
                c,
                cache,
                worklist,
                call_retargets,
            );
        }
        c.generic_call_sites.extend(inner_sites);
        c.arity_pad_count.extend(inner_pads);
        if checked.is_err() || c.errors.len() > saved_errors {
            // The original body passed; a clone that does not is a
            // face this step cannot mint soundly — keep the call channel.
            c.errors.truncate(saved_errors);
            owned_ast
                .stmts
                .retain(|s| !matches!(s, Stmt::FnDecl { name, .. } if *name == face));
            c.closure_fn_names.remove(&face);
            c.widen_clone_origin.remove(&face);
        } else {
            minted = true;
        }
    }
    minted
}

/// A spec body's copy of a construction site that reads `new.target`
/// is such a site too (`check_monomorph_closures` clones the body and
/// its lifted closures per specialization), or a function expression
/// inside an untyped function never gets its face.
pub(crate) fn carry_sites(ast: &mut Ast, id_map: &[(ExprId, ExprId)]) {
    let carried: Vec<ExprId> = id_map
        .iter()
        .filter(|(old, _)| ast.new_target_fn_exprs.contains(old))
        .map(|&(_, new)| new)
        .collect();
    ast.new_target_fn_exprs.extend(carried);
}
