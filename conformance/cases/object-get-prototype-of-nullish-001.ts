// `Object.getPrototypeOf(nullish)` — ES §20.1.2.12 step 1 requires
// ToObject(obj) BEFORE any [[GetPrototypeOf]] read, and ToObject
// throws TypeError on `null` / `undefined`. Pre-fix tr silently
// returned a null Any across every route (typed nullish literal fell
// through to the `_` arm's ANY_NULL; a runtime-Any nullish read
// `__proto__` off a nullish box and unbox'd to null; neither
// production path complied).
//
// Fix: the getPrototypeOf lowering probes the checker type of
// `args[0]`. Compile-time known `Type::Null` / `Type::Undefined`
// unconditionally emits `throw new TypeError` via the pre-existing
// runtime helper `__torajs_anyv_throw_typeerror_if_props_nullish`.
// A `Type::Any` / `Type::Nullable(T)` arg goes through the same
// gate before the runtime reflection route — the gate is a no-op
// when the boxed value is non-nullish, so the existing path
// through `__torajs_get_proto_of_any` still runs for real objects.
//
// test262 unblocks: Object/getPrototypeOf/15.2.3.2-1-2 (the direct
// null-throws assertion).

// Compile-time nullish literals — both must throw a TypeError.
try {
  Object.getPrototypeOf(null)
  console.log("null: NO_THROW (BAD)")
} catch {
  console.log("null: thrown")
}

try {
  Object.getPrototypeOf(undefined)
  console.log("undef: NO_THROW (BAD)")
} catch {
  console.log("undef: thrown")
}

// Typed non-nullish primitives — the throw gate MUST NOT fire; the
// call falls through to the runtime classifier and answers whatever
// prototype tr wires up (return type is Any; only presence / absence
// of a throw is asserted here so tr-vs-bun stays bun-parity).
let ok = 0
try { Object.getPrototypeOf(0); ok += 1 } catch { }
try { Object.getPrototypeOf("hi"); ok += 1 } catch { }
try { Object.getPrototypeOf(true); ok += 1 } catch { }
try { Object.getPrototypeOf({}); ok += 1 } catch { }
try { Object.getPrototypeOf([]); ok += 1 } catch { }
console.log("typed non-throw count:", ok)     // 5

// any-lane — the runtime gate must catch the nullish path even
// when the arg is not a literal.
let anyNull: any = null
try {
  Object.getPrototypeOf(anyNull)
  console.log("any null: NO_THROW (BAD)")
} catch {
  console.log("any null: thrown")
}

let anyUndef: any = undefined
try {
  Object.getPrototypeOf(anyUndef)
  console.log("any undef: NO_THROW (BAD)")
} catch {
  console.log("any undef: thrown")
}

// any-lane over a real object — the gate lets it through.
let anyObj: any = {}
let objOk = 0
try { Object.getPrototypeOf(anyObj); objOk = 1 } catch { }
console.log("any object non-throw:", objOk)     // 1

// Trailing-arg drop still evaluates for side effects (spec
// L-to-R). The extra arg's `console.log` fires even when the
// primary arg triggers the throw.
let sideEffect = 0
function bump(): number { sideEffect += 1; return 999 }
try {
  Object.getPrototypeOf(null, bump() as any)
  console.log("side-effect: NO_THROW (BAD)")
} catch {
  console.log("side-effect: thrown, bump ran:", sideEffect)   // 1
}
