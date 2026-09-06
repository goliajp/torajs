//! WHICH argument position the spec calls with no receiver — the slot
//! table behind `bind_fnexpr_this_default`, split out at the 500-line
//! cap in rotation 417. The parent answers "how is the binding made";
//! this file answers "where does the spec say `undefined`", which is
//! the question every new entry has to be read off spec text to
//! settle.
//!
//! The table is a WHITELIST, not "whatever is left": a call this file
//! does not name answers with no positions at all, which leaves it on
//! its loud reject. A wrong `undefined` is the silent wrong the design
//! principles rank worst, and the reverted blanket version of this
//! pass produced exactly that for `arr.forEach(fn, obj)`.

use super::fnexpr_this_default::Certain;
use super::{Ast, Expr, ExprId};

/// The prototype methods that both take no-receiver handlers and
/// answer a promise, so a chain over them stays certain.
pub(super) const HANDLER_METHODS: [&str; 3] = ["then", "catch", "finally"];

/// Array iteration methods whose callback NEVER takes a thisArg —
/// the spec hands them `undefined` whatever the call site wrote.
const ARRAY_NO_THISARG: [&str; 4] = ["sort", "toSorted", "reduce", "reduceRight"];

/// Array iteration methods whose callback takes an OPTIONAL thisArg
/// as the second argument: `undefined` is the answer only when the
/// call site omitted it.
const ARRAY_OPTIONAL_THISARG: [&str; 10] = [
    "forEach",
    "map",
    "filter",
    "some",
    "every",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flatMap",
];

/// The argument positions of `recv.<name>(args…)` that the spec
/// invokes with no receiver — empty for every call this table does
/// not name, which is what keeps an unaudited slot on its loud
/// reject.
pub(super) fn no_receiver_slots(
    ast: &Ast,
    certain: &Certain,
    obj: ExprId,
    name: &str,
    args: &[ExprId],
) -> Vec<ExprId> {
    // §7.3.35 GroupBy step 4.c — `Call(callback, undefined, «value,
    // key»)`, reached through `Object.groupBy` / `Map.groupBy`.
    if name == "groupBy"
        && matches!(&ast.exprs[obj.0 as usize], Expr::Ident(n) if n == "Object" || n == "Map")
    {
        return args.iter().skip(1).take(1).copied().collect();
    }
    // proposal-array-from-async §2.1.1 step 5.e — `Call(mapfn,
    // thisArg, «v, k»)`, where thisArg is the THIRD argument. So
    // `undefined` is the answer only when the call site omitted it,
    // the same arity rule the optional-thisArg array methods use. A
    // WRITTEN thisArg keeps the loud reject: the map kernel
    // (`__torajs_array_from_async_map_dyn`) takes `(items, mapfn)`
    // and the lowering eval-and-drops args[2..], so there is nothing
    // to thread the object through yet (L3b). `Array.from`'s mapFn is
    // not here — `collect_array_from_face` promotes that one, thisArg
    // and all, and it gates on the method name.
    if name == "fromAsync" && matches!(&ast.exprs[obj.0 as usize], Expr::Ident(n) if n == "Array") {
        return if args.len() == 2 {
            args.iter().skip(1).take(1).copied().collect()
        } else {
            Vec::new()
        };
    }
    // §22.1.3.19 String.prototype.replace / replaceAll — both spell
    // the replacer call as `Call(replaceValue, undefined, «matched,
    // …, string»)`, and so does the regexp route they delegate to
    // (§22.2.6.11 RegExp.prototype[@@replace] step 14.a). Two things
    // have to be certain, and neither of them is a spelling: that
    // `String.prototype`'s method is the one that runs, and that the
    // searchValue is not a user object whose own `@@replace` would
    // decide how the replacer gets called. A LITERAL searchValue —
    // regexp or string — settles the second; [`Certain::string`]
    // settles the first for a binding as well as for a literal.
    //
    // The receiver test used to be `Expr::String(_)` alone, and the
    // two spellings of one program therefore disagreed:
    // `'xQx'.replace('Q', f)` ran and `const s = 'xQx';
    // s.replace('Q', f)` was an unclaimed-receiver refusal, as were
    // both regexp spellings over a binding. Over-refusal only — the
    // answer here is `undefined`, which is what the literal spelling
    // was already supplying.
    //
    // The literal receiver with a string searchValue never reaches
    // this arm: `fnexpr_this_cb_slots::collect_replace_face` PROMOTES
    // that one (the str replace_fn lane has a receiver slot and the
    // site seeds `undefined` into it), and a claimed slot is filtered
    // out before this table's answer is used. Same answer, earlier
    // route.
    if matches!(name, "replace" | "replaceAll")
        && certain.string(&ast.exprs, obj)
        && args.first().is_some_and(|p| {
            matches!(
                &ast.exprs[p.0 as usize],
                Expr::Regex { .. } | Expr::String(_)
            )
        })
    {
        return args.iter().skip(1).take(1).copied().collect();
    }
    // proposal-upsert `Map.prototype.getOrInsertComputed` /
    // `WeakMap.prototype.getOrInsertComputed` — the computed branch is
    // `Call(callbackfn, undefined, «key»)`, one argument and no
    // thisArg anywhere in the signature. The receiver has to be a
    // certain Map / WeakMap for the same reason the promise handlers
    // need a certain promise: a user object with a method of that name
    // decides for itself how it calls what it is handed.
    if name == "getOrInsertComputed" && certain.map(&ast.exprs, obj) {
        return args.iter().skip(1).take(1).copied().collect();
    }
    if HANDLER_METHODS.contains(&name) && certain.promise(&ast.exprs, obj) {
        // `then` takes two handler slots; `catch` / `finally` one.
        // Anything past them is not a handler and is left alone.
        let slots = if name == "then" { 2 } else { 1 };
        return args.iter().take(slots).copied().collect();
    }
    if !certain.array(&ast.exprs, obj) {
        return Vec::new();
    }
    let admits = ARRAY_NO_THISARG.contains(&name)
        || (ARRAY_OPTIONAL_THISARG.contains(&name) && args.len() == 1);
    if admits {
        args.iter().take(1).copied().collect()
    } else {
        Vec::new()
    }
}
