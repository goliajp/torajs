// T-28 — Default param missing → undefined (per ES spec §10.2.1.4).
// JS supplies `undefined` to any param the caller didn't pass. Pre-T-28
// tora rejected such calls at typecheck with "expected N argument(s),
// got M". With T-28, calls with fewer args are allowed when the
// trailing missing params are all `Type::Any` — typed-tier params
// keep strict arity (typed slots can't hold undefined).
//
// Implementation: check.rs records `arity_pad_count[Call ExprId] =
// missing_count` when the trailing missing params are all Any.
// ssa_lower's Call arm reads this and emits ANY_UNDEF Any-box operands
// for each missing trailing position so the SSA argv aligns with the
// callee's static signature.

function f(a: any, b: any, c: any) {
  console.log(a);
  console.log(b);
  console.log(c);
  console.log(typeof c);
}

// Single missing trailing (1 → 2 params).
f(1);                   // 1, undefined, undefined, undefined
// Single missing trailing (2 → 3 params).
f(1, 2);                // 1, 2, undefined, undefined
// All present.
f(1, 2, 3);             // 1, 2, 3, number

// Mixed types in args (Any param accepts anything per JS).
function g(x: any, y: any) {
  console.log(x);
  console.log(y);
}
g("hi");                // hi, undefined
g("hi", true);          // hi, true

// Default-undef in nested call (caller provides 0 args; callee's
// 1-param signature receives undefined).
function h(z: any) {
  console.log(typeof z);
  return z;
}
h();                    // undefined
console.log(typeof h()); // undefined
