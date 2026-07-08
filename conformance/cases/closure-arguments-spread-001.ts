// RFC 20260708-spread-call chunk 2b — `...arguments` inside a
// full-arguments closure body.
//
// The EscapingTouch scan absorbs a spread whose source is the bare
// `arguments` ident (the argv face serves it from the materialized
// array), so such bodies qualify for the boxed dual entry instead
// of staying KeepLoud. The rewrite Call / Array arms then swap the
// spread source to `__torajs_arguments`: call-position spreads ride
// apply_spread_args' guarded index-read expansion (chunk 1) with
// the Any→scalar admit + coerce + F64 width (chunk 2a); array-
// literal spreads ride the existing Arr<Any> literal-spread lane.
// Named-fn / declared-pair bodies keep the inline params expansion
// (declared == actual there). Killed chains (escaping aliases,
// unsafe pass-through returns) still reject loudly.

function sum3(a: number, b: number, c: number): number { return a + b + c; }

// call-position spread — exact arity.
const f = function () { return sum3(...arguments); };
console.log(f(10, 20, 12));                   // 42

// beyond-arity call — excess elements ignored by the fixed-arity
// callee (JS semantics).
console.log(f(1, 2, 3, 4, 5));                // 6

// fixed prefix arg before the spread.
const h = function () { return sum3(0, ...arguments); };
console.log(h(20, 22));                       // 42

// array-literal spread — arguments copy.
const g = function () { const copy: any[] = [...arguments]; return copy.length; };
console.log(g(1, 2, 3));                      // 3

// mixed elems around the spread.
const m = function () { const xs: any[] = [...arguments, 99]; return xs.length; };
console.log(m(1, 2));                         // 3
