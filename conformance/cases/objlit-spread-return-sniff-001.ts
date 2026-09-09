// The parser encodes an object-literal SPREAD as a sentinel field
// name — `__spread__`, or `__spread_omit__:p,q` for the
// destructuring-rest form — and parks the source expression. What it
// contributes is the SOURCE's own keys, a runtime fact for anything
// but a statically known Struct.
//
// The implicit return-annotation sniff spelled the sentinel straight
// through, so an unannotated function returning `{...n}` was given a
// return type naming a field literally called `__spread__`. The
// DECLARED spelling said so out loud —
//
//     function f(n: {a:number}) { return {...n}; }
//         type error: return type mismatch: function expects
//         Struct([("__spread__", Struct([("a", Number)]))]),
//         got Struct([("a", Number)])
//
// — while the unannotated one did not: `f(1)` answered
// `{"__spread__":0}` and `f({a:1})` / `f("ab")` SEGFAULTED, the
// caller reading a layout the callee never built. The computed-key
// sentinel one line above already takes the `any` exit; this is its
// twin.

function spread(n: any) {
  return { ...n };
}
console.log(JSON.stringify(spread(1)), JSON.stringify(spread(true)));
console.log(JSON.stringify(spread({ a: 1 })), JSON.stringify(spread("ab")));
console.log(JSON.stringify(spread(1n)), JSON.stringify(spread(null)));

function spreadPlus(n: any) {
  return { ...n, z: 9 };
}
console.log(JSON.stringify(spreadPlus(1)), JSON.stringify(spreadPlus({ a: 1 })));

// the declared receiver takes the same exit
function declared(n: { a: number }) {
  return { ...n };
}
console.log(JSON.stringify(declared({ a: 1 })));

// two call sites specialize the same body
function twice(n: any) {
  return { ...n };
}
console.log(JSON.stringify(twice({ a: 1 })), JSON.stringify(twice({ a: 2 })));

// the destructuring-rest sentinel (`__spread_omit__:`) rides along
function rest(n: any) {
  const { a, ...r } = n;
  return { ...r };
}
console.log(JSON.stringify(rest({ a: 1, b: 2 })));
