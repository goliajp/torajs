// T-28-substrate — Default param missing → undefined for UNTYPED JS
// params (the common shape in test262). `function f(a, b)` gets
// rewritten by `desugar_implicit_generics` into `function f<T0, T1>
// (a: T0, b: T1)` — fresh independent TypeVars per param. Pre-T-28-
// substrate the trailing-missing relaxation was skipped on this path
// because `type_to_ann(Type::Any) → "number"` collapsed Any into the
// Number i64 mono slot, and the padded ANY_UNDEF Any-box would land
// in an i64 slot the callee read as garbage.
//
// T-28-substrate fix: type_to_ann emits "any" for Type::Any so the
// generic mono cache produces a real Any-typed specialization (e.g.
// `f$$_number_any`) whose param slot ABI matches the box ptr. The
// generic-arity branch in check.rs binds trailing TypeVars to
// Type::Any when independent (don't appear in earlier params or
// return type) and records arity_pad_count for ssa_lower.

function f(a, b, c) {
  console.log(a);
  console.log(b);
  console.log(c);
  console.log(typeof c);
}

// 1 → 3 params: pad b and c with ANY_UNDEF.
f(1);                  // 1, undefined, undefined, undefined
// 2 → 3 params: pad c.
f(1, "two");           // 1, two, undefined, undefined
// All present.
f(1, "two", true);     // 1, two, true, boolean

// Single-param fn called with 0 args.
function g(x) {
  console.log(typeof x);
}
g();                   // undefined

// Mixed type literals in earlier args.
function h(a, b) {
  console.log(typeof a);
  console.log(typeof b);
}
h("hi");               // string, undefined
h(42, true);           // number, boolean
