//! W4 — the frozen analysis result: [`WidthTable`], the read-only
//! query surface every lowering site asks (`slot_is_f64` /
//! `elem_is_f64` / `field_is_f64`, the nominal-alias and fallthrough
//! registries, the bounds-proof verdicts). Moved out of
//! `container.rs` verbatim (rotation 642): that file keeps the
//! walk-time machinery — the union-find, the congruence closure, the
//! guarded-edge activation and the canonicalization — and this one
//! keeps what survives it.

use super::SlotKey;
use super::container::{UnionFind, canon_key_frozen};
use crate::ast::ExprId;
use crate::ast::PropKey;
use std::collections::{HashMap, HashSet};

/// Frozen analysis result: the poison fixpoint over canonical alias
/// classes (scalar slots and container elem/field points in one
/// lattice), queried through the frozen union-find so any spelling of
/// a key resolves to its class representative.
pub(crate) struct WidthTable {
    canon: HashSet<SlotKey>,
    /// W-ESC — frozen class reps whose containers flow into `any`-
    /// annotated slots (escape.rs); the widen site re-interns their
    /// Arr elems as Type::Any.
    any_escaped: HashSet<SlotKey>,
    /// The arrays of callback-taking iterations whose class is in
    /// `any_escaped` (`hof_receivers.rs`).
    escaped_hof_receivers: HashSet<ExprId>,
    /// The declarations to retype `any` and the call arguments to
    /// widen (`hidden_any.rs`).
    pub(crate) hidden_any: super::HiddenAny,
    uf: UnionFind,
    container_poison: bool,
    /// The classes a promise can reach unseen (`promise_entry`): a
    /// promise value point they hold reads at f64.
    pub(super) promise_untracked: HashSet<SlotKey>,
    /// D5 — cyclic plain-alias names (see `alias.rs`); the TypeDecl
    /// fill site takes nominal widths for these, like classes.
    nominal_aliases: HashSet<String>,
    /// RFC 20260725-fallthrough-return knives 1-2 — functions with a
    /// path that runs off the end of the body, which answers
    /// `undefined` there. The call site asks here to know a result
    /// read off one of them may hold that answer's sentinel.
    fallthrough_fns: HashSet<String>,
    /// The mirror of `fallthrough_fns`: `(fn name, param name)` pairs
    /// some call site hands the `undefined` sentinel. The parameter's
    /// own body asks here, because the value arrives from a caller
    /// lowered separately and nothing else records it.
    undef_sentinel_params: HashSet<(String, String)>,
    /// Container bindings some write fills an ELEMENT of with the
    /// sentinel. A for-of over one binds a loop variable the body
    /// reads by name, and the binding registry the body consults is
    /// per-function — so the answer has to come from here, the way a
    /// tainted parameter's does.
    sentinel_elems: HashSet<String>,
    /// Field names some write fills with the sentinel. The lowering
    /// side keeps its own mirror of this question
    /// (`crate::undef_f64_fields`), but that mirror is primed per
    /// function context and asks its own predicate, whose `Ident`
    /// arm can only see bindings of the function being lowered. A
    /// constructor's `this.v = v` therefore marked nothing when the
    /// read `b.v` was lowered somewhere else, and `new Box(zs[9])`
    /// read back a plain number. This is the analysis's answer,
    /// which is by name and program-wide, handed over as a seed.
    sentinel_fields: HashSet<PropKey>,
    /// The index reads the walk proved in-bounds — `xs[i]` under an
    /// enclosing, untainted `i < xs.length` guard (see
    /// [`crate::ssa_lower_bounds_proven`]). It rides here rather than
    /// beside the table because the element-width verdicts above are
    /// only sound under this very proof: the reason a `number`
    /// element widens to F64 is that an unproven read owes
    /// `undefined`, which an I64 slot cannot spell. A consumer that
    /// can reach the widths can therefore always reach the proof they
    /// were taken under, and cannot substitute another.
    index_read_proven: HashSet<ExprId>,
    /// The subset of those under a pair that settles both ends — the
    /// read lands inside `[0, length)` and so never answers
    /// `undefined`. The throw analysis asks it: a member read off such
    /// an element cannot reach the nullish guard's raise.
    index_read_settled: HashSet<ExprId>,
    /// The `s.charCodeAt(i)` calls the walk proved in range — the
    /// same guard pair with both ends settled — and therefore gave
    /// an integer width instead of the F64 a NaN answer needs. Rides
    /// here for the same reason as the index reads: the lowering
    /// that drops the NaN arm has to read the proof the width was
    /// taken under.
    char_code_at_proven: HashSet<ExprId>,
    /// The integer `+ - *` whose every result stays inside the exact
    /// window (`magnitude.rs`); the rest compute in f64.
    arith_in_window: HashSet<ExprId>,
    /// The operands of the negations proven not to see a zero
    /// (`nonzero_neg`): the width walk kept their integer face, so the
    /// lowering has to emit an integer subtract for them.
    nonzero_negs: HashSet<ExprId>,
    /// The pre-walk slot-name registry (`analyze_tables::
    /// collect_slot_registry`): every binding's spellings by name —
    /// `Param(f, n)` / `Local(f, n)` per top-level fn, `Global(n)` for
    /// a top-level let. The walk's `resolve` answered a name off this
    /// registry; [`WidthTable::ident_keys`] answers a lowering the same
    /// way, so a reader asks for the very key the analysis populated
    /// rather than guessing from its own slot table (where a captured
    /// top-level name sits at prologue depth like a param).
    by_name: HashMap<String, Vec<SlotKey>>,
    /// The bindings each lifted closure's captures resolved to — the
    /// walk's `resolve` consults it ahead of the top-level spelling, so
    /// [`WidthTable::ident_keys`] has to as well.
    captures: super::captures::CaptureMap,
    bound_closures: super::bound_closures::BoundClosures,
}

impl WidthTable {
    #[allow(clippy::too_many_arguments)] // frozen-analysis assembly, one call site
    pub(super) fn new(
        canon: HashSet<SlotKey>,
        escapes: (HashSet<SlotKey>, HashSet<ExprId>, super::HiddenAny),
        uf: UnionFind,
        container_poison: bool,
        nominal_aliases: HashSet<String>,
        fallthrough_fns: HashSet<String>,
        undef_sentinel_params: HashSet<(String, String)>,
        sentinel_elems: HashSet<String>,
        sentinel_fields: HashSet<PropKey>,
        index_read_proven: HashSet<ExprId>,
        index_read_settled: HashSet<ExprId>,
        char_code_at_proven: HashSet<ExprId>,
        arith_in_window: HashSet<ExprId>,
        nonzero_negs: HashSet<ExprId>,
        by_name: HashMap<String, Vec<SlotKey>>,
        captures: super::captures::CaptureMap,
        bound_closures: super::bound_closures::BoundClosures,
    ) -> Self {
        let (any_escaped, escaped_hof_receivers, hidden_any) = escapes;
        WidthTable {
            canon,
            any_escaped,
            escaped_hof_receivers,
            hidden_any,
            uf,
            container_poison,
            promise_untracked: HashSet::new(),
            nominal_aliases,
            fallthrough_fns,
            undef_sentinel_params,
            sentinel_elems,
            sentinel_fields,
            index_read_proven,
            index_read_settled,
            char_code_at_proven,
            arith_in_window,
            nonzero_negs,
            by_name,
            captures,
            bound_closures,
        }
    }

    /// The key the walk's `resolve` filed a read of `n` from `fn_name`
    /// under (`""` for the top level): the fn's own param, else its
    /// own local, else the binding its creator handed it, else the
    /// top-level binding; from a lifted closure (`__`-prefixed) a name
    /// none of those pin is the `Captured` key the walk unioned every
    /// same-named slot onto. Empty when the analysis never saw the
    /// name — no class to ask.
    pub(crate) fn ident_keys(&self, fn_name: &str, n: &str) -> Vec<SlotKey> {
        let Some(spellings) = self.by_name.get(n) else {
            return Vec::new();
        };
        let has = |k: &SlotKey| spellings.contains(k);
        if !fn_name.is_empty() {
            let param = SlotKey::Param(fn_name.to_string(), n.to_string());
            if has(&param) {
                return vec![param];
            }
            let local = SlotKey::Local(fn_name.to_string(), n.to_string());
            if has(&local) {
                return vec![local];
            }
            if let Some(c) = self.captures.get(fn_name).and_then(|m| m.get(n)) {
                return vec![
                    c.clone()
                        .unwrap_or_else(|| SlotKey::Captured(n.to_string())),
                ];
            }
        }
        let global = SlotKey::Global(n.to_string());
        if has(&global) {
            return vec![global];
        }
        if fn_name.starts_with("__") {
            return vec![SlotKey::Captured(n.to_string())];
        }
        Vec::new()
    }

    /// Every lifted closure an identifier read may denote
    /// (`bound_closures.rs`); empty when it names none.
    pub(crate) fn bound_closures(&self, eid: ExprId) -> &[String] {
        self.bound_closures.get(&eid).map_or(&[], |c| &c.fns)
    }

    /// See the field's doc — the guard-dominated bounds proof the
    /// element widths were decided under.
    pub(crate) fn index_read_proven(&self, eid: ExprId) -> bool {
        self.index_read_proven.contains(&eid)
    }

    /// See the field's doc — the reads whose pair settles both ends.
    pub(crate) fn index_read_settled(&self, eid: ExprId) -> bool {
        self.index_read_settled.contains(&eid)
    }

    /// See the field's doc — the `charCodeAt` calls whose width was
    /// narrowed to an integer under the bounds proof.
    pub(crate) fn char_code_at_proven(&self, eid: ExprId) -> bool {
        self.char_code_at_proven.contains(&eid)
    }

    /// See the field's doc — an integer `+ - *` that may keep the i64
    /// path.
    pub(crate) fn arith_in_window(&self, eid: ExprId) -> bool {
        self.arith_in_window.contains(&eid)
    }

    /// See the field's doc — keyed by the negation's operand.
    pub(crate) fn neg_operand_nonzero(&self, operand: ExprId) -> bool {
        self.nonzero_negs.contains(&operand)
    }

    /// True when `param` of `fn_name` may be handed the `undefined`
    /// sentinel by one of its call sites, so the consumers inside that
    /// body have to check for it the way they check a binding that was
    /// initialized from the same shape.
    pub(crate) fn param_takes_undef_sentinel(&self, fn_name: &str, param: &str) -> bool {
        self.undef_sentinel_params
            .contains(&(fn_name.to_string(), param.to_string()))
    }

    /// True when some write filled an element of the container bound
    /// to `name` with a sentinel-bearing value, so a loop variable
    /// taken off it may hold one.
    /// The analysis's sentinel field names, for the lowering mirror
    /// to seed itself with (see the field doc).
    pub(crate) fn sentinel_fields(&self) -> &HashSet<PropKey> {
        &self.sentinel_fields
    }

    pub(crate) fn container_elem_takes_sentinel(&self, name: &str) -> bool {
        self.sentinel_elems.contains(name)
    }

    pub(crate) fn is_nominal_alias(&self, name: &str) -> bool {
        self.nominal_aliases.contains(name)
    }

    /// RFC 20260725-fallthrough-return knife 1 — true when calling
    /// `name` can answer `undefined` by running off the end of its
    /// body (ES §10.2.1.4 step 11) rather than through a `return`.
    pub(crate) fn returns_undef_on_fallthrough(&self, name: &str) -> bool {
        self.fallthrough_fns.contains(name)
    }

    pub(crate) fn slot_is_f64(&self, k: &SlotKey) -> bool {
        self.canon.contains(&canon_key_frozen(&self.uf, k))
    }

    /// W-ESC — true when the container held in slot `k` flows into
    /// the `any` world (its frozen class rep carries an escape face).
    pub(crate) fn slot_escapes_any(&self, k: &SlotKey) -> bool {
        !self.any_escaped.is_empty() && self.any_escaped.contains(&canon_key_frozen(&self.uf, k))
    }

    /// Is `recv`, the array of a callback-taking iteration, one whose
    /// class escapes into the `any` world?
    pub(crate) fn hof_receiver_escapes(&self, recv: ExprId) -> bool {
        self.escaped_hof_receivers.contains(&recv)
    }

    pub(crate) fn elem_is_f64(&self, holder: &SlotKey) -> bool {
        self.container_poison || self.slot_is_f64(&SlotKey::Elem(Box::new(holder.clone())))
    }

    /// Is the value point of the promise slot `holder` f64: widened by
    /// a writer, or held by a class a promise can reach unseen?
    pub(crate) fn promise_value_is_f64(&self, holder: &SlotKey) -> bool {
        self.field_is_f64(holder, "value")
            || super::promise_entry::holder_untracked(&self.promise_untracked, &self.uf, holder)
    }

    #[allow(dead_code)] // D3 wires the struct/class field face
    pub(crate) fn field_is_f64(&self, holder: &SlotKey, name: impl Into<PropKey>) -> bool {
        self.container_poison
            || self.slot_is_f64(&SlotKey::Field(Box::new(holder.clone()), name.into()))
    }
}
