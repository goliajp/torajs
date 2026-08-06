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

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId};

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

/// Walk the whole module once and answer what it might shadow.
///
/// Recomputed per function from the caller's own `Ast`, the same
/// shared-set contract as [`crate::dynobj_degrade`] and
/// [`crate::let_widen`]: both sides read one snapshot, so an `ExprId`
/// rewritten between check and lower cannot strand the result.
pub(crate) fn collect_shadowed_builtin_methods(ast: &Ast) -> ShadowSet {
    let mut set = ShadowSet::default();

    // Every `<expr>.prototype` occurrence, with the family it names.
    let mut proto_exprs: HashMap<ExprId, Family> = HashMap::new();
    // Reverse edge: base ExprId -> the accesses naming it as object.
    let mut bases: HashMap<ExprId, Vec<Access>> = HashMap::new();
    let mut assign_targets: HashSet<ExprId> = HashSet::new();
    let mut delete_targets: HashSet<ExprId> = HashSet::new();
    // `Object.defineProperty(P, "m", d)` — a write that reaches the
    // prototype without naming a member of it, so the base-use rule
    // above would read it as an escape. Attribute it instead.
    let mut define_calls: Vec<(ExprId, Option<String>)> = Vec::new();
    // Every mention of a builtin constructor BY NAME, and the subset
    // of expressions consumed in a position that only reads through
    // the name. A mention outside that subset means the constructor
    // VALUE went somewhere this scan cannot follow — `const A = Array`
    // being the plain case, `f(Array)` the other one.
    let mut ctor_idents: HashMap<ExprId, Family> = HashMap::new();
    let mut consumed: HashSet<ExprId> = HashSet::new();

    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Ident(name) => {
                if let Some(f) = BUILTIN_CTORS.iter().copied().find(|c| *c == name.as_str()) {
                    ctor_idents.insert(eid, f);
                }
            }
            Expr::Member { obj, name } => {
                if name == "prototype" {
                    if let Some(f) = builtin_named_by(ast, *obj) {
                        proto_exprs.insert(eid, f);
                    }
                }
                consumed.insert(*obj);
                bases
                    .entry(*obj)
                    .or_default()
                    .push(Access::Named(eid, name.clone()));
            }
            Expr::Index { obj, index } => {
                let key = str_literal(ast, *index);
                if key.as_deref() == Some("prototype")
                    && let Some(f) = builtin_named_by(ast, *obj)
                {
                    proto_exprs.insert(eid, f);
                }
                consumed.insert(*obj);
                bases.entry(*obj).or_default().push(match key {
                    Some(k) => Access::Named(eid, k),
                    None => Access::Computed(eid),
                });
            }
            // The remaining positions where naming a builtin
            // constructor does NOT hand its value to anyone: calling
            // it, reading through it, asking its type, casting it.
            Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. } => {
                consumed.insert(*obj);
            }
            Expr::OptCall { callee, .. } | Expr::NewDynamic { callee, .. } => {
                consumed.insert(*callee);
            }
            Expr::TypeOf { expr } | Expr::As { expr, .. } => {
                consumed.insert(*expr);
            }
            Expr::Assign { target, .. } => {
                assign_targets.insert(*target);
            }
            Expr::Delete { expr } => {
                delete_targets.insert(*expr);
            }
            Expr::Call { callee, args } => {
                consumed.insert(*callee);
                if let Some(kind) = reflective_callee(ast, *callee) {
                    match kind {
                        // Hands out a prototype object with no syntactic
                        // spelling of which one.
                        Reflective::GetProto => set.all = true,
                        Reflective::Define => {
                            if let Some(&target) = args.first() {
                                define_calls
                                    .push((target, args.get(1).and_then(|a| str_literal(ast, *a))));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // A builtin constructor whose name is mentioned anywhere other
    // than "read something through it" has escaped: `const A = Array`
    // makes `A.prototype.join = f` a patch this scan never sees,
    // because `A` is not a name it recognises. Today that lands the
    // program in L0 and the patch is IGNORED — silently, which is the
    // one outcome the design does not accept.
    //
    // Standing the whole family down is the sound answer and it is
    // cheap to be right about: the escaped name tells us WHICH family,
    // so this widens one family rather than reaching for `all`. Being
    // wrong here (a mention that could not have led to a patch) costs
    // that family its typed tier in that program — a slower program,
    // never a wrong one. `new Array(n)` and `x instanceof Array` are
    // not affected at all: both spell the constructor as a plain
    // String in the AST, not as an `Ident` expression.
    for (eid, family) in &ctor_idents {
        if !consumed.contains(eid) {
            set.widen(family);
        }
    }

    for (&proto, &family) in &proto_exprs {
        let mut attributed = false;

        for (target, key) in &define_calls {
            if *target == proto {
                attributed = true;
                match key {
                    Some(m) => {
                        set.methods.insert((family, m.clone()));
                    }
                    None => set.widen(family),
                }
            }
        }

        for access in bases.get(&proto).into_iter().flatten() {
            attributed = true;
            match access {
                // A read cannot install a patch; only a write or a
                // delete of this member can.
                Access::Named(member, name) => {
                    if assign_targets.contains(member) || delete_targets.contains(member) {
                        set.methods.insert((family, name.clone()));
                    }
                }
                Access::Computed(member) => {
                    if assign_targets.contains(member) || delete_targets.contains(member) {
                        set.widen(family);
                    }
                }
            }
        }

        // Parent is neither a member access nor a define call: the
        // prototype went somewhere this pass cannot follow.
        if !attributed {
            set.widen(family);
        }
    }

    set
}

/// One access naming some expression as its object.
enum Access {
    Named(ExprId, String),
    Computed(ExprId),
}

enum Reflective {
    GetProto,
    Define,
}

/// `Object.getPrototypeOf` / `Reflect.defineProperty` / siblings.
fn reflective_callee(ast: &Ast, callee: ExprId) -> Option<Reflective> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Object" && ns != "Reflect" {
        return None;
    }
    match name.as_str() {
        "getPrototypeOf" | "setPrototypeOf" => Some(Reflective::GetProto),
        "defineProperty" | "defineProperties" => Some(Reflective::Define),
        _ => None,
    }
}

/// The builtin constructor an expression names, if it names one
/// directly. Anything else — an alias, a user class, a call result —
/// answers `None`, which the caller reads as "unattributable family".
fn builtin_named_by(ast: &Ast, obj: ExprId) -> Option<Family> {
    let Expr::Ident(name) = ast.get_expr(obj) else {
        return None;
    };
    BUILTIN_CTORS.iter().copied().find(|c| *c == name.as_str())
}

/// The string a literal key spells, for `p["join"]` shapes.
fn str_literal(ast: &Ast, e: ExprId) -> Option<String> {
    match ast.get_expr(e) {
        Expr::String(s) => Some(s.clone()),
        _ => None,
    }
}
