//! Step hookups: a for-of binding reads the parser's `src[i]` element
//! key, while the value the loop delivers is the `value` field of what
//! the iterator's `next()` answers (§7.4.8 IteratorValue). These join
//! the two, so the binding's width follows the value it is given.

use super::super::Analysis;
use super::super::slot_keys::SlotKeyId;

/// Generator step hookup: `for (const v of h(3))` binds `v` to the
/// ELEMENT of the generator object, and the parser's pre-built
/// `src[i]` element read spells that as `Elem(Ret(h))`. The value
/// actually delivered is the `value` field of what `h`'s desugared
/// class answers from `next()`, and nothing joined the two — so a
/// widened step value met a narrow `nums` at `nums.push(v)` and the
/// write refused loudly ("container width analysis missed this
/// write", 553-04).
///
/// `generator_factory_classes` is the factory-fn → `__Gen_<name>` map
/// `desugar_generators` leaves behind; the step methods are found the
/// same way `nominal_unions` finds a class's methods, by prefix over
/// the analyzed fn names, so both the direct and the any-lane copy
/// join.
pub(in crate::num_width) fn generator_step_unions(a: &mut Analysis) {
    if a.ast.generator_factory_classes.is_empty() {
        return;
    }
    let mut fn_names: Vec<String> = a.fn_params.keys().cloned().collect();
    fn_names.sort();
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    let factories: Vec<(String, String)> = a
        .ast
        .generator_factory_classes
        .iter()
        .map(|(f, c)| (f.clone(), c.clone()))
        .collect();
    for (factory, class) in factories {
        let fr = a.keys.ret(&factory);
        let elem = a.keys.elem(fr);
        for m in &fn_names {
            if *m == format!("__cm_{class}__next") || *m == format!("__cmany_{class}__next") {
                let mr = a.keys.ret(m);
                unions.push((elem, a.keys.field(mr, "value")));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}

/// Iterator step hookup, the class-protocol counterpart of
/// [`generator_step_unions`]: `for (const v of it)` over an instance of
/// a class `C` whose `[Symbol.iterator]()` declares iterator class `I`
/// binds `v` through the parser's `src[i]` element read, which spells
/// `Elem(Class(C))`, while the loop loads `value` out of what `I`'s
/// `next()` answers (§7.4.8 IteratorValue). With nothing joining the
/// two, a fractional `value` met an integer-wide `out` at
/// `out.push(v)`. Both copies of `next` join, as above.
pub(in crate::num_width) fn iterator_step_unions(a: &mut Analysis) {
    use crate::ssa_lower_for_of_iter_protocol_plan::{iterator_method_classes, iterator_next_fn};
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    for st in &a.ast.stmts {
        let Some((src, iter)) = iterator_method_classes(a.ast, st) else {
            continue;
        };
        let elem = a.keys.elem(a.keys.class(src));
        let next = iterator_next_fn(iter);
        for m in [next.clone(), next.replacen("__cm_", "__cmany_", 1)] {
            if a.fn_params.contains_key(&m) {
                let mr = a.keys.ret(&m);
                unions.push((elem, a.keys.field(mr, "value")));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}
