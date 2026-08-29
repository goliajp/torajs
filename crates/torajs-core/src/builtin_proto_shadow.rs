//! Which builtin-prototype methods this module might monkey-patch —
//! the whole-program fact that lets the typed tier stay straight-line.
//!
//! RFC 20260806-typed-tier-proto-shadow. The measured problem: a typed
//! receiver never reaches the anyvalue method dispatcher, so the patch
//! bitmap and the delete tombstones the dispatcher consults are invisible
//! to it. 96 of 96 probes across `number[]` / `string[]` / `string` /
//! `number` receivers missed a patch that every `any`-typed receiver saw.
//!
//! The fix is not a runtime guard at each site — torajs is AOT with no
//! deopt tier, and 96 diamonds is unconditional artifact cost paid by
//! every program to serve one almost no program performs. It is a
//! compile-time stand-down: the typed lowering arms decline for the
//! `(family, method)` pairs this module says are reachable, and the
//! call is typed `any` instead, which routes it through the arm that
//! already exists for a concrete receiver whose member read answers Any
//! (`ssa_lower_any_method_call`'s cluster-#4 branch) — receiver boxing,
//! argv and the ownership account are all that arm's, already built.
//! Nothing new is emitted. The runtime bitmap decides, which keeps the
//! ordering exact — a call sequenced *before* the patch still answers
//! from the kernel, because the bitmap is read when the call runs, not
//! when it is compiled.
//!
//! # Soundness
//!
//! A builtin prototype is only mutable by a program that can first name
//! it, and it can only be named through a member access spelled
//! `prototype`, or through `getPrototypeOf`. A program containing
//! neither holds no builtin prototype at all, so its typed tier is
//! unconditionally safe — that is the zero-cost common
//! case, and it is why `bench/`'s 87 files (none of which mention any of
//! those spellings) compile to the same bytes as before.
//!
//! Each syntactic occurrence of `X.prototype` is its own `ExprId` with
//! exactly one parent, so "is this occurrence used as a member base"
//! decides escape exactly: if some `Member`/`Index` names it as the
//! object, that access *is* its parent and nothing else can see it; if
//! none does, its parent is something else — a binding, an argument, a
//! return — and the prototype has escaped where we can no longer track
//! writes to it, so its family stands down wholesale.
//!
//! # Known residue (pre-existing, not introduced here)
//!
//! `__proto__` is deliberately not a trigger. The desugar passes emit
//! their own `__proto__` member reads to wire the injected builtin
//! classes' prototype chains (`ast/class_globals_register.rs`), and
//! nothing in the node shape tells those apart from a user's — treating
//! it as one stood every program's typed tier down globally, which is
//! how this was found. So `o.__proto__.join = f` keeps today's
//! behaviour, the same as the aliasing case below.
//!
//! A builtin reached under another name — `const A = Array; A.prototype
//! .join = f`, or `[].constructor` through a dynamic key — is not
//! detected, because tracking it is escape analysis and treating every
//! unknown identifier's prototype as a builtin's would stand the typed
//! tier down for every program that defines a class. Such a program
//! keeps today's behaviour on typed receivers. This narrows an existing
//! hole rather than opening one: before this pass, *every* typed
//! receiver missed *every* patch.

use std::collections::HashSet;

/// The module walk that fills a [`ShadowSet`]. A child module so it
/// writes this one's private fields unchanged.
mod scan;

pub(crate) use scan::collect_shadowed_builtin_methods;

/// Constructors whose `.prototype` carries methods the typed tier
/// lowers directly. An identifier outside this list is a user class or
/// an alias; a write to its prototype cannot shadow a builtin method,
/// and an alias that hides a builtin is the residue documented above.
const BUILTIN_CTORS: &[&str] = &[
    "Array", "String", "Number", "Boolean", "Date", "RegExp", "Map", "Set", "WeakMap", "WeakSet",
    "Promise", "Function", "Object", "BigInt", "Symbol",
];

/// The builtin families a receiver can belong to, spelled exactly as the
/// constructor is — the scan keys on the source name and the gate maps a
/// receiver type onto the same spelling, so the two cannot drift apart.
pub(crate) type Family = &'static str;

/// What a module may shadow, at the coarsest granularity that is still
/// honest about what was proven.
#[derive(Default)]
pub(crate) struct ShadowSet {
    /// Something unattributable happened — every family stands down.
    all: bool,
    /// Families whose method set could not be pinned down (a computed
    /// key, or a prototype value that escaped this pass's reach).
    families: HashSet<Family>,
    /// The precise pairs: `Array.prototype.join = f` names exactly one.
    methods: HashSet<(Family, String)>,
}

impl ShadowSet {
    /// True when the program provably cannot reach any builtin
    /// prototype — the case the typed tier is free in.
    pub(crate) fn is_empty(&self) -> bool {
        !self.all && self.families.is_empty() && self.methods.is_empty()
    }

    /// Families the fallback lane is known to serve — an allowlist, not
    /// a denylist, because two sweeps showed the gaps are wherever we
    /// have not looked rather than in one identifiable place.
    ///
    /// Opening every family at once moved 40 test262 cases from `pass`
    /// to `incompatible:not yet supported`; excluding the
    /// `Array.prototype` higher-order methods moved a different 19
    /// (`Promise.allSettled` / `Promise.any` / the async iterator
    /// prototypes). Chasing that family by family is one ~10-minute
    /// sweep per round with no reason to expect the next round to be
    /// the last. So the rule is inverted: stand down only where a probe
    /// has actually shown the dispatcher answering, which is exactly
    /// the families `bypass_probe.py` covers. A patch on anything else
    /// keeps today's behaviour — wrong, but no worse than before, and
    /// never at the cost of a program's build.
    ///
    /// Widening this list is not a one-line change. The order is fixed
    /// and was learned the expensive way: teach the fallback lane to
    /// serve the family, prove it with a probe, and only then let the
    /// gate open. Opening first is what produced both regressions
    /// above. Promise was added once the closure-cell wrap and the
    /// argument-typing fix had removed both reasons the lane could not
    /// serve a stood-down call — a sweep then showed its 19 regressions
    /// gone.
    /// Each family joined only once its probe read BYPASS on the typed
    /// receiver *and* `ok` on the `<any>` control — the control is what
    /// pins the miss on the typed tier rather than on the dispatcher,
    /// and `tr-err` of 0 is what says the probe ran at all (a wrapper
    /// that cannot execute measures nothing, which is how Promise's
    /// first reading came back empty).
    ///
    /// **BigInt is deliberately absent.** Its typed rows bypass, but so
    /// do its `<any>` rows: the dispatcher does not consult the bitmap
    /// for a bigint receiver either, so standing the typed tier down
    /// would hand those calls to a lane that answers no better. The
    /// gap is real and still open — it is just not this gate's to fix,
    /// and opening here would claim a fix the probe does not show.
    const MEASURED_FAMILIES: &[Family] = &[
        "Array", "String", "Number", "Promise", "Map", "Set", "Date", "RegExp", "Boolean",
        "Symbol", "WeakMap", "WeakSet",
    ];

    /// Should the typed tier stand down for this call?
    ///
    /// `Object` counts for every receiver: every builtin prototype ends
    /// its own [[Prototype]] chain at `Object.prototype`, so a patch
    /// there is reachable from an array, a string and a number alike.
    pub(crate) fn shadows(&self, family: Family, method: &str) -> bool {
        Self::MEASURED_FAMILIES.contains(&family) && self.reaches(family, method)
    }

    /// Does a write this module performs reach `family`'s `method`?
    fn reaches(&self, family: Family, method: &str) -> bool {
        self.all
            || self.families.contains(family)
            || self.families.contains("Object")
            || self.methods.contains(&(family, method.to_string()))
            || self.methods.contains(&("Object", method.to_string()))
    }

    /// The builtin methods that take a function argument. Only these
    /// can push a fn-name argument into an any-boxed argv slot when
    /// the call stands down, so only these earn the wrap.
    ///
    /// The restriction is not tidiness — it is correctness. Wrapping
    /// rewrites a function name into a `__forward_*` cell, which
    /// changes the value's *identity*, and `Object.getPrototypeOf`
    /// sets [`Self::all`], so an unrestricted question answers yes at
    /// every member call in any program that calls it. That wrapped
    /// `Object.getPrototypeOf(asyncGenFn)` and made
    /// %AsyncGeneratorFunction% answer as %GeneratorFunction% — two
    /// gate fixtures, caught the first time this axis ran.
    const CALLBACK_TAKING: &[&str] = &[
        "map",
        "filter",
        "reduce",
        "reduceRight",
        "forEach",
        "some",
        "every",
        "flatMap",
        "find",
        "findIndex",
        "findLast",
        "findLastIndex",
        "sort",
    ];

    /// Might a call to `method` stand down *and* carry a callback?
    ///
    /// The pre-typecheck fn-to-closure wrap
    /// ([`crate::ast_collect_fn_closure`]) needs this before any
    /// receiver has a type, so it asks the question unquantified by
    /// family: a yes on any measured family is a yes for every
    /// receiver at that method name. Over-answering costs one closure
    /// wrap in a program that already patches a builtin prototype;
    /// under-answering costs that program its build.
    pub(crate) fn may_stand_down(&self, method: &str) -> bool {
        Self::CALLBACK_TAKING.contains(&method)
            && Self::MEASURED_FAMILIES
                .iter()
                .any(|&f| self.reaches(f, method))
    }

    fn widen(&mut self, family: Family) {
        self.families.insert(family);
    }
}

/// The receiver's builtin family, for the gate. `None` = a receiver
/// whose methods the typed tier owns outright (a struct, a closure) or
/// one already on the any-lane, where the dispatcher runs anyway.
/// Keyed on the checker's type rather than the SSA one so the gate can
/// answer before the receiver is lowered — the decision is whether to
/// enter the lowering arms at all.
///
/// `Any` answers `None` on purpose: such a receiver already reaches the
/// dispatcher, which does its own consult.
/// Might this module have changed what turning an ARRAY into a string
/// does?
///
/// The gate above serves method calls, which the checker stands down
/// by typing the callee `any`. A COERCION has no callee to type:
/// `String(xs)`, `xs + ""`, a template substitution and `Number(xs)`
/// fold straight to the join kernel, so they kept answering "1,2"
/// while `xs.toString()` right next to them answered the patch.
///
/// Two names, because §7.1.17 resolves `toString` on the receiver and
/// §23.1.3.36 then resolves `join` — the direct kernel is that whole
/// program only while neither has been touched. `Object` counts for
/// both through `reaches`, since the walk ends there.
pub(crate) fn arr_to_string_shadowed(set: &ShadowSet) -> bool {
    !set.is_empty() && (set.shadows("Array", "toString") || set.shadows("Array", "join"))
}

pub(crate) fn family_of(ty: &crate::check::Type) -> Option<Family> {
    use crate::check::Type;
    Some(match ty {
        Type::Array(_) => "Array",
        Type::String => "String",
        Type::Number => "Number",
        Type::Boolean => "Boolean",
        Type::Date => "Date",
        Type::RegExp => "RegExp",
        Type::Map => "Map",
        Type::Set => "Set",
        Type::WeakMap => "WeakMap",
        Type::WeakSet => "WeakSet",
        Type::Promise(_) => "Promise",
        Type::BigInt => "BigInt",
        Type::Symbol => "Symbol",
        _ => return None,
    })
}
