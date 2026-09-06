//! The fn-name registry rows — Step 2 of the `__torajs_fn_name_table`
//! pipeline, plus the symbol-spelling strippers that decide what a
//! row's user-visible name is.
//!
//! Split out of [`super::body_passes`] verbatim in rotation 595. That
//! file answers *how the four body-lowering passes are sequenced*;
//! this one answers *what the fn-name table is told about each body*,
//! and it was 288 of the file's 480 prod lines — no room left for the
//! `__fwdrecv_` arm the same rotation needed to add.

use crate::ast::{Ast, PropKey, unmangle_key};
use crate::ssa::{self, FnNameEntry, FuncId, Module};
use std::collections::HashMap;

/// Fn-name registry Step 2 — record the (FuncId, name, name_sid)
/// triple for the link-time __torajs_fn_name_table emit (Step 3) +
/// the runtime __torajs_fn_print_inline binary search (Step 4).
/// Skip the desugared mangled forms (`__dispatch_<m>`, `__new_<C>`)
/// — bun reports the user-visible method name on those, not the
/// mangled name, and we get there in Step 5's wire by stripping the
/// prefix when emitting. Skip generic-mono specialized names too
/// (`<fn>__<typeargs>__<idx>`) — they share the source fn's
/// user-visible name; the entry already exists for the generic form.
/// Closure-lifted bodies (`__closure_*`) are anonymous from the
/// user's point of view; runtime falls back to
/// `[Function (anonymous)]` if no entry is found.
///
/// RFC 20260719-fn-tostring-source B6c — a dispatchable class-method
/// body (`__cm_<C>__<m>` with a synthesized boxed adapter) registers
/// against its ADAPTER's fn id: the reified `C.prototype.<m>` face
/// cell carries the adapter vaddr (its own fn_addr is the throwing
/// native entry), so toString/name/length resolve the user-visible
/// row through it. Ctor bodies, accessor bodies (their face carries
/// its own name/length meta), and adapter-less dropouts stay out.
/// A monomorphized instance's symbol carries a `$$` suffix — the
/// all-`any` clone is `$$anywv` (`check_monomorph_any_widen`), a
/// typed instance `$$_<type>`. The ES name is the source spelling,
/// so the suffix comes off before the row is written: a generic
/// class's method printed `[Function: get2$$anywv]` and answered
/// that from `.name`, where bun answers `get2`. Only a real
/// instance suffix is stripped — a property key may itself contain
/// `$$` (`class C { "a$$b"() {} }`), and its tail is neither
/// `anywv` nor `_`-led.
pub(crate) fn strip_mono_suffix(name: &str) -> &str {
    match name.rsplit_once("$$") {
        Some((base, tail)) if tail == "anywv" || tail.starts_with('_') => base,
        _ => name,
    }
}

/// The property key of the accessor pair whose GET or SET body is the
/// lifted fn `name`, instance or static — `None` for every other fn
/// (566-02). The body's symbol spells `<p>_get` / `<p>_set`, and a
/// property may itself end in `_get`, so the key comes from the pair
/// registry rather than from the spelling.
fn accessor_prop_of<'a>(ast: &'a Ast, name: &str) -> Option<&'a PropKey> {
    ast.accessor_getters
        .iter()
        .chain(ast.accessor_setters.iter())
        .chain(ast.static_accessor_getters.iter())
        .chain(ast.static_accessor_setters.iter())
        .find(|(_, f)| *f == name)
        .map(|((_, p), _)| p)
}

/// ES-spec `Function.length` — leading params before the first default
/// or rest (§10.2.10 SetFunctionLength), with the two synthetic slots
/// dropped.
///
/// `__env` is the closure environment every lowered fn value carries.
/// `__this` is the hidden receiver `bind_this_param` inserts into a
/// declaration whose body says `this`, and which `__fwdrecv_`
/// re-declares; every caller able to reach such a value shifts argv
/// past it on `FLAG_CLOSURE_RECV_FIRST`, so it is no more part of the
/// user's arity than `__env` is. Counting it answered `2` for a
/// one-parameter fn on every receiver-honouring position — the
/// `__cm_` arm had always shed both, and this is now the one copy of
/// that rule rather than two that could drift apart.
fn es_arity(params: &[crate::ast::Param]) -> u32 {
    params
        .iter()
        .filter(|p| p.name != "__env" && p.name != "__this")
        .take_while(|p| p.default.is_none() && !p.is_rest)
        .count() as u32
}

pub(super) fn register_fn_name(
    module: &mut Module,
    name: &str,
    params: &[crate::ast::Param],
    fid: FuncId,
    ast: &Ast,
    span: crate::lexer::Span,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    let class_parents = &ast.class_parents;
    if name.starts_with("__cm_") {
        let Some(mname) = strip_mangled_method_name(name, "__cm_", class_parents) else {
            return;
        };
        if mname == "ctor" || mname.is_empty() {
            return;
        }
        let Some(&(adapter_fid, _)) = boxed_entries.get(&fid) else {
            return;
        };
        // 566-02 — an ACCESSOR body's symbol is `<p>_get` / `<p>_set`,
        // so the key comes from the pair registry rather than the
        // spelling (a property may itself end in `_get`). The row
        // exists for the INSPECT face: bun prints `[Function: cg]`
        // for `get cg() {}`, the name in the SOURCE, while `.name`
        // answers the §10.2.9 prefixed `"get cg"` off the reified
        // face — which is read first, so the row cannot shadow it.
        // A COMPUTED accessor has no source name at all and keeps
        // the anonymous `[Function]` form, the same verdict 564-01
        // gave the computed method face — 567-03 gives it an EMPTY
        // row rather than no row, because the two things a row
        // carries are separable: no NAME, but the source TEXT
        // `toString` owes it (§20.2.3.5), which the miss used to
        // answer with the erased native form.
        let key = match accessor_prop_of(ast, name) {
            Some(p) if p.starts_with("__ccm_") => PropKey::from(""),
            Some(p) => p.clone(),
            // The symbol spelling is a bijection of the key (557-02 C
            // 组), so the user-visible name comes back exactly, lone
            // surrogate and all — after the mono-instance suffix
            // comes off.
            None => unmangle_key(strip_mono_suffix(mname)),
        };
        let lit = ssa::StringLiteral::encode_from_wtf8(&key);
        let name_sid = ssa::StringId(module.strings.len() as u32);
        module.strings.push(lit);
        let arity = es_arity(params);
        let (src_sid, src_len) = intern_fn_source(module, ast, span);
        module.fn_name_globals.push(FnNameEntry {
            fn_id: adapter_fid,
            name: key,
            name_sid,
            arity,
            src_sid,
            src_len,
        });
        return;
    }
    if name.starts_with("__dispatch_")
        || name.starts_with("__new_")
        || name.starts_with("__closure_")
        || name.starts_with("__bind_create_")
        || name.contains("__mono_")
    {
        return;
    }
    // `__forward_<target>` fn-value wrappers (ast_closure_param_tag)
    // carry the user-visible TARGET name — `const t: any = topfn;
    // t.name` must answer "topfn", and the synthetic leading `__env`
    // param stays out of the arity (chunk 716). `__bound_<fn>_<id>`
    // bind-desugar wrappers (ast_desugar_function_prototype_methods)
    // carry the ES SetFunctionName form `bound <fn>` (§20.2.3.2) —
    // chunk 798; the trailing `_<id>` disambiguator is stripped. The
    // `__bind_create_<fn>_<id>` factory twins are call-only synthetics
    // the user never holds a value of, so they skip the table above.
    let bound_form = name
        .strip_prefix("__bound_")
        .and_then(|rest| rest.rsplit_once('_'))
        .map(|(target, _id)| format!("bound {target}"));
    let visible = bound_form.as_deref().unwrap_or_else(|| {
        // `__fwdrecv_<target>` is the receiver-FIRST twin of that same
        // wrapper (`ast::namedfn_recv_cb`), minted wherever the
        // read-back channel honours a receiver, and it carries the
        // user-visible target name for the same reason: before
        // rotation 595 a stored `o.m = t` answered
        // `[Function: __fwdrecv_t]` and `.name === "__fwdrecv_t"` —
        // a synthetic identifier the user cannot even spell.
        let base = name
            .strip_prefix("__forward_")
            .or_else(|| name.strip_prefix("__fwdrecv_"))
            .unwrap_or(name);
        // 423-01 deconflict — a module-mangled decl (`__m<k>_<name>`)
        // answers the user spelling, mirroring the namespace object's
        // FIELD face (`B.tag.name` must say "tag").
        let base = strip_module_mangle(base);
        // RFC 20260729-fn-value-any V4 刀 2 — a hoisted generator
        // EXPRESSION answers the NamedEvaluation verdict the hoist
        // pass recorded (binding name, or its own self-name, or the
        // empty ES name), never the `__genexpr_N` mint. Generator
        // DECLARATIONS never land a row here and keep their own name.
        if let Some(n) = ast.genexpr_names.get(base) {
            return n.as_str();
        }
        // §B.3.3's hoist renames a nested `function f` so two blocks
        // can each declare one; the mint is not a name any source can
        // spell, and `.name` answered it verbatim
        // (`__nested_outer_Foo_0` where bun says `Foo`). The lift
        // records what it renamed, because the symbol cannot be taken
        // apart again — parent and name may both contain `_`.
        if let Some(n) = ast.nested_fn_names.get(base) {
            return n.as_str();
        }
        base
    });
    // `__sm_<C>__<M>` static-method bodies carry the ES
    // SetFunctionName form — the property key `<M>` (`K.sf.name`
    // answered the mangled name). `<C>` is matched against the
    // known class set (longest first) since both a class name and
    // a method name may themselves contain `__`; the key comes back
    // out of the symbol spelling exactly (557-02 C 组).
    // 562-09 — a generic function / static method lowers only as its
    // mono instance, so the symbol reaching here is the suffixed one.
    let visible = strip_mono_suffix(visible);
    // 566-02 — a STATIC accessor body reaches this arm as
    // `__sm_<C>__<p>_get`, and stripping only the class prefix left
    // the `_get` on: `[Function: sg_get]` where bun prints the source
    // spelling `sg`. Same registry lookup the instance arm makes, and
    // 567-03's empty-name row for a computed one.
    let visible: PropKey = match accessor_prop_of(ast, name) {
        Some(p) if p.starts_with("__ccm_") => PropKey::from(""),
        Some(p) => p.clone(),
        None => match strip_static_method_name(visible, class_parents) {
            Some(m) => unmangle_key(m),
            None => PropKey::from(visible),
        },
    };
    // Intern the name as a Module-level string literal so the link
    // layer can resolve `__user_string_<sid>` to the rodata cstring
    // entry. The encoder picks Latin-1 / UTF-16 to match the
    // upstream string-literal encoding contract (TS allows non-ASCII
    // fn names).
    let lit = ssa::StringLiteral::encode_from_wtf8(&visible);
    let name_sid = ssa::StringId(module.strings.len() as u32);
    module.strings.push(lit);
    let arity = es_arity(params);
    let (src_sid, src_len) = intern_fn_source(module, ast, span);
    module.fn_name_globals.push(FnNameEntry {
        fn_id: fid,
        name: visible.clone(),
        name_sid,
        arity,
        src_sid,
        src_len,
    });
    // 398-05 — a STATIC method body registers a second row against
    // its ADAPTER fid, the `__cm_` mirror: the class-object own entry
    // (`__torajs_class_static_method_define`) is a reified cell
    // carrying the `__sm_` adapter's vaddr, and the any-lane
    // `.length` / `.name` / toString reads resolve through
    // `registry_addr` = that adapter. The body-fid row above stays —
    // the compile-time-folded `S.s` read path answers off it.
    if name.starts_with("__sm_")
        && let Some(&(adapter_fid, _)) = boxed_entries.get(&fid)
    {
        module.fn_name_globals.push(FnNameEntry {
            fn_id: adapter_fid,
            name: visible,
            name_sid,
            arity,
            src_sid,
            src_len,
        });
    }
}

/// RFC 20260719-fn-tostring-source B3b — intern the type-erased
/// source slice for a registry row. Sentinel (0,0) spans (synthesized
/// decls with no user-written source) answer `(None, 0)`, which the
/// link layer bakes as a NULL `src_ptr`.
pub(crate) fn intern_fn_source(
    module: &mut Module,
    ast: &Ast,
    span: crate::lexer::Span,
) -> (Option<ssa::StringId>, u32) {
    if span.start == 0 && span.end == 0 {
        return (None, 0);
    }
    let erased = crate::fn_source_erase::erase_types(&ast.source, &ast.type_ann_spans, span);
    let lit = ssa::StringLiteral::encode_from_str(&erased);
    let src_len = lit.length;
    let src_sid = ssa::StringId(module.strings.len() as u32);
    module.strings.push(lit);
    (Some(src_sid), src_len)
}

/// `__sm_<C>__<M>` → `<M>` when `<C>` is a declared class name.
/// Longest class-name match wins — `class A__B { static f() {} }`
/// desugars to `__sm_A__B__f`, where a shortest-match would answer
/// `B__f`. Shared with the static `.name` member fold
/// (`ssa_lower_member_fn_intro`), which sees the same mangled ident
/// after the checker rewrites `K.sf` to `Ident("__sm_K__sf")`.
/// `__m<k>_<name>` → `<name>` — the 423-01 module-deconflict mangle
/// shape (`__m` + decimal sequence + `_`). Anything else — including
/// other `__m…` synthetics whose next byte is not a digit — passes
/// through untouched.
pub(crate) fn strip_module_mangle(n: &str) -> &str {
    let Some(rest) = n.strip_prefix("__m") else {
        return n;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return n;
    }
    match rest[digits..].strip_prefix('_') {
        Some(user) if !user.is_empty() => user,
        _ => n,
    }
}

pub(crate) fn strip_static_method_name<'a>(
    name: &'a str,
    class_parents: &HashMap<String, Option<String>>,
) -> Option<&'a str> {
    strip_mangled_method_name(name, "__sm_", class_parents)
}

/// `<prefix><C>__<M>` → `<M>` when `<C>` is a declared class name,
/// longest class-name match winning (see
/// [`strip_static_method_name`]). B6c generalization — the `__cm_`
/// instance-method registry rows strip the same mangled shape.
pub(crate) fn strip_mangled_method_name<'a>(
    name: &'a str,
    prefix: &str,
    class_parents: &HashMap<String, Option<String>>,
) -> Option<&'a str> {
    let rest = name.strip_prefix(prefix)?;
    class_parents
        .keys()
        .filter_map(|c| {
            rest.strip_prefix(c.as_str())
                .and_then(|r| r.strip_prefix("__"))
                .map(|m| (c.len(), m))
        })
        .max_by_key(|(clen, _)| *clen)
        .map(|(_, m)| m)
}
