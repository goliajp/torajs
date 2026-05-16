// T-31 — `arguments.length` must reflect real call-site argc, not the
// fn's declared param count. Pre-fix tora folded `arguments.length` to
// `Number(<declared arity>)` at desugar time, so missing trailing Any
// params (T-28 pad) and missing args all reported declared arity. Bun
// (and ES spec) reports the actual count passed by the caller.
//
// This fixture exercises Any-typed params so T-28's "pad missing
// trailing slots with undefined" path is in play. Each call deliberately
// supplies fewer args than declared so the difference between declared
// arity and real argc is observable.

function f(a: any, b: any, c: any): any {
  return arguments.length;
}

// declared 3, called with 1 → bun: 1, pre-fix tora: 3
console.log(f(7));

// declared 3, called with 2 → bun: 2, pre-fix tora: 3
console.log(f(7, 8));

// declared 3, called with 3 (exact) → bun: 3 (matches lucky)
console.log(f(7, 8, 9));

// Zero-arg call against a 1-param fn.
function g(x: any): any {
  return arguments.length;
}
console.log(g());

// Nested fn — outer is called with 2 args, inner sees its own argc.
function outer(a: any, b: any): any {
  function inner(p: any, q: any, r: any): any {
    return arguments.length;
  }
  return inner(a, b) + arguments.length;
}
// outer's arguments.length=2 + inner's arguments.length=2 → 4
console.log(outer(10, 20));
