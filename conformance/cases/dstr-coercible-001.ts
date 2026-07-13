// RFC 20260714-dstr-residual blade 1 — ES §13.3.3.5 RequireObjectCoercible:
// an object binding pattern throws TypeError when its source is null /
// undefined — including the empty pattern (which reads no members) and
// nested patterns (whose intermediate source may be null at any depth).

// empty pattern, param position, null
function f0({}) {}
try {
  f0(null);
  console.log("f0 no-throw");
} catch (e) {
  console.log("f0:", e instanceof TypeError, e.name);
}

// empty pattern, param position, undefined (via any-typed variable —
// a literal `undefined` argument hits a pre-existing ssa-lower ann gap,
// recorded in RFC 20260714-dstr-residual)
let u: any = undefined;
try {
  f0(u);
  console.log("f0u no-throw");
} catch (e) {
  console.log("f0u:", e instanceof TypeError, e.name);
}

// nested pattern with default, inner source null
function f1({ w: { x } = { x: 4 } }) {
  console.log("f1 x=", x);
}
try {
  f1({ w: null });
  console.log("f1 no-throw");
} catch (e) {
  console.log("f1:", e instanceof TypeError, e.name);
}
f1({ w: { x: 9 } });
f1({} as any);

// let position, empty pattern, null source
try {
  let {} = null as any;
  console.log("let no-throw");
} catch (e) {
  console.log("let:", e instanceof TypeError, e.name);
}

// let position, undefined source with bindings
try {
  let src: any = undefined;
  let { p } = src;
  console.log("letp no-throw", p);
} catch (e) {
  console.log("letp:", e instanceof TypeError, e.name);
}

// catch-parameter position, empty pattern, thrown null
try {
  try {
    throw null;
  } catch ({}) {}
  console.log("catch no-throw");
} catch (e) {
  console.log("catch:", e instanceof TypeError, e.name);
}

// normal paths keep working
let src2: any = { a: 1 };
let { a, b = 5 } = src2;
console.log("norm:", a, b);
function g({ x, y: { z } }: any) {
  console.log("g:", x, z);
}
g({ x: 7, y: { z: 8 } });
let {} = { q: 1 };
console.log("done");
