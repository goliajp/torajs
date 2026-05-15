// T-27.a — Function as Object (Closure-form). Per ECMAScript §10.2
// Function values are first-class objects: `f.x = v` and `f.x` are
// spec-required (used heavily by Array/iterator polyfills, React
// class component static methods, jQuery plugin patterns). ~231
// test262 cases blocked pre-T-27 on "field assignment target must
// be a struct, got Function".
//
// Implementation: extend closure env layout with a lazy props_dynobj
// field at offset 24 (between drop_fn and capture base). NULL on
// construction; on first `f.x = v` allocates a dynobj and writes
// the ptr. The closure's env_drop_fn calls value_drop_heap on the
// props field at scope exit so the dynobj is freed deterministically
// (TAG_DYNOBJ dispatch in __torajs_value_drop_heap walks all live
// buckets). Cap base shifted to offset 32 — all SSA cap reads/writes
// derive from CLOSURE_CAP_BASE_OFF so this is mechanical.
//
// Closure-form constructions only here. Top-level FnDecl and non-
// capturing function expressions (Type::FnSig at SSA layer; raw fn
// pointer, no env) are a separate path (T-27.b followup) — they
// need either lazy-promote-to-Closure (pre-pass scanning Member
// access on fn-typed idents) or a side table keyed by fn_addr.

let token = 100;
let f = function() { return token + 1; };

// First write — lazy-allocs the dynobj, stores at CLOSURE_PROPS_OFF.
f.x = 7;
console.log(f.x);             // 7
console.log(typeof f.x);      // number

// Multiple properties — same dynobj.
f.name2 = "alice";
f.flag = true;
console.log(f.name2);         // alice
console.log(f.flag);          // true

// Overwrite same key — dynobj_set replaces in-place (no rebucket).
f.x = 99;
console.log(f.x);             // 99

// Read of unset prop — fn_props_get returns ANY_UNDEF box.
console.log(typeof f.unset);  // undefined

// Function still callable — fn_addr at CLOSURE_FN_ADDR_OFF unchanged.
console.log(f());             // 101
