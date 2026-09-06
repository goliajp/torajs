// A top-level binding holding a variadic ARROW could not be called
// from inside any function. Two passes disagreed about one fact.
//
// The canonical `__fn(...)->(R)` spelling a lifted closure registers
// under was built by joining its parameters' own annotations — and a
// rest parameter's own annotation is the ARRAY spelling (`number[]`);
// what makes it a rest is the flag beside it. So `(...xs: number[])
// => number` registered as one Array parameter, arity 1, and every
// consumer believed that. Separately, the call arm folded a rest
// sentinel back to the packed array whenever the callee was a global
// and not a local — but the packer that does the packing keys on
// rest-declaring FnDecls, which a `const` binding is not. Inside a
// named fn body the binding is exactly "a global that is not a
// local", so the fold fired on a call nobody had packed, and three
// loose arguments met one Array parameter.
//
// At top level the binding IS a local, so every one of these worked
// there — which is why the whole family read as a return-boundary
// nuisance rather than what it was.

const ga = (...xs: number[]): number => xs.length;

function callIt(): number {
  return ga(1, 2, 3);
}
function callNone(): number {
  return ga();
}
function callMany(): number {
  return ga(1, 2, 3, 4, 5, 6, 7);
}
console.log(callIt(), callNone(), callMany(), ga(1, 2));

// a fixed prefix ahead of the tail, including a call that stops right
// after the prefix
const gm = (a: number, ...xs: number[]): number => a + xs.length;
function withPrefix(): number {
  return gm(9, 1, 2);
}
function prefixOnly(): number {
  return gm(9);
}
console.log(withPrefix(), prefixOnly());

// the function-expression spelling of the same value
const gf = function (...xs: number[]): number {
  return xs.length;
};
function callFnExpr(): number {
  return gf(1, 2);
}
console.log(callFnExpr());

// a capture in the body, a mutable binding, a spread call site, and a
// call from a nested function
const k = 10;
const gk = (...xs: number[]): number => xs.length + k;
let gl = (...xs: number[]): number => xs.length;
const arr = [1, 2, 3];
function nested(): number {
  function inner(): number {
    return gk(1, 2);
  }
  return inner();
}
function viaSpread(): number {
  return ga(...arr);
}
function callMutable(): number {
  return gl(1, 2);
}
console.log(nested(), viaSpread(), callMutable());

// a `var` binding read from a named fn body is the same reader as a
// promoted one, even though `var` never promotes: routing its call
// statically segfaulted
var gvar = (...xs: number[]): number => xs.length;
function callVar(): number {
  return gvar(1, 2, 3);
}
console.log(callVar(), gvar(1));

// a local of the same name still shadows the global and keeps its own
// route
function shadowed(): number {
  const ga = (a: number): number => a * 2;
  return ga(5);
}
console.log(shadowed());

// the body reading its packed tail as an array
const sum = (...xs: number[]): number => {
  let t = 0;
  for (const x of xs) {
    t += x;
  }
  return t;
};
function callSum(): number {
  return sum(1, 2, 3, 4);
}
console.log(callSum());
