// P3.3 — `Object.defineProperty(obj, key, { value: v })` per
// ES spec §10.1.6.4 OrdinaryDefineOwnProperty. Routes the
// `.value` field of the descriptor through dynobj_set. Other
// descriptor fields (writable / configurable / enumerable / get
// / set) are subset-deferred — only `.value` is honored, which
// matches the common test262 idiom of using defineProperty as
// a sealed-ish setter on a non-class object.
//
// Pre-P3.3 tora hard-rejected at typecheck with 'Object.
// defineProperty not supported in nominal class system; planned
// for T-27'. Test262 uses this pervasively (~541 cases blocked
// across the broader sample). Now the typecheck accepts and
// ssa_lower intercepts the Call shape:
//   Object.defineProperty(<any>, <string>, { value: <expr> })
// → extract dynobj from Any-box (offset 16) → pack <expr> as
//   (tag, value) → call dynobj_set with the key.
//
// Other descriptor shapes (no .value, get/set, multi-field) fall
// through to the generic Call path which still rejects (Type::
// Function arity check) — they need richer descriptor handling
// in a follow-up substrate piece.

let x: any = {}
Object.defineProperty(x, "foo", { value: 1 })
console.log(x.foo)                            // 1
console.log(x.foo === 1)                      // true

// Different value types via defineProperty.
let y: any = {}
Object.defineProperty(y, "n", { value: 42 })
Object.defineProperty(y, "s", { value: "hello" })
Object.defineProperty(y, "b", { value: true })
Object.defineProperty(y, "z", { value: null })
console.log(y.n)                              // 42
console.log(y.s)                              // hello
console.log(y.b)                              // true
console.log(y.z)                              // null

// Pre-existing dynobj key still readable after defineProperty
// adds new keys.
let z: any = {}
z.original = "first"
Object.defineProperty(z, "added", { value: "second" })
console.log(z.original)                       // first
console.log(z.added)                          // second

// defineProperty overwrites an existing key.
let w: any = {}
w.foo = 1
Object.defineProperty(w, "foo", { value: 99 })
console.log(w.foo)                            // 99
