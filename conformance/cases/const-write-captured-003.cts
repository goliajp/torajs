// More scopes a write to an outer `const` reaches from a nested
// function (§9.1.1.1.5 SetMutableBinding on an immutable binding
// throws a TypeError, sloppy too, after the right-hand side ran), and
// the bindings that shadow it there.
//
// - A declaration list: `const c = 1, d = () => { c = 2 }` shares one
//   scope, so `d` writes the `c` declared beside it.
// - An instance field initializer runs in the class body's scope
//   (§15.7.10), not the constructor's: a constructor parameter or
//   local of the same name does not shadow the outer binding there,
//   and the initializer's own locals do.
// - A parameter default and a static field initializer are function
//   code of their own; a parameter of the same name shadows.
// - A class body is strict code (§11.2.2), so a method writing the
//   self-name of an enclosing sloppy generator expression throws,
//   where the same write from a sloppy arrow is ignored (§15.5.5).
//
// Oracle is node's script goal, pinned in `.expected`: bun's
// transpiler refuses some of these assignments statically.
const m1 = 1, w1 = () => { m1 = 2; };
try { w1(); console.log("no throw", m1); } catch (e) { console.log("multi", e.name, m1); }
let m2 = 1, w2 = () => { m2 = 2; };
w2();
console.log("multi-let", m2);
const m3 = 1, w3 = () => { let m3 = 0; m3 = 2; return m3; };
console.log("multi-shadow", w3(), m3);
const c5 = 1;
let x5 = 0, f5 = () => { c5 = (x5 = 1, 2); };
try { f5(); console.log("no throw", c5); } catch (e) { console.log("multi-let-group", e.name, c5, x5); }
function inFn() {
  const a = 1, b = function () { a = 2; };
  try { b(); console.log("no throw", a); } catch (e) { console.log("multi-fn", e.name, a); }
  const p = 1, q = function (p) { p = 3; return p; };
  console.log("multi-fn-param", q(0), p);
}
inFn();

const k = 1;
class A {
  f = () => { k = 2; };
  constructor(k) { console.log("ctor-param", k); }
}
try { new A(5).f(); console.log("no throw", k); } catch (e) { console.log("field-vs-ctor-param", e.name, k); }
class B {
  f = () => { k = 3; };
  constructor() { let k = 9; k = 10; console.log("ctor-local", k); }
}
try { new B().f(); console.log("no throw", k); } catch (e) { console.log("field-vs-ctor-local", e.name, k); }
class C2 {
  f = () => { let k = 0; k = 4; return k; };
  constructor(k) {}
}
console.log("field-own-local", new C2(5).f(), k);

const c3 = 1;
function g(f = () => { c3 = 2; }) { f(); }
try { g(); console.log("no throw", c3); } catch (e) { console.log("default", e.name, c3); }
function g2(c3, f = () => { c3 = 2; return c3; }) { return f(); }
console.log("default-param-shadow", g2(0), c3);
const ar = (f = () => { c3 = 3; }) => f();
try { ar(); console.log("no throw", c3); } catch (e) { console.log("arrow-default", e.name, c3); }
class M { m(f = () => { c3 = 4; }) { f(); } }
try { new M().m(); console.log("no throw", c3); } catch (e) { console.log("method-default", e.name, c3); }
function gg(f = () => { const z = 1; (() => { z = 2; })(); return z; }) { return f(); }
try { console.log("no throw", gg()); } catch (e) { console.log("const-in-default", e.name); }
class S { static f = () => { c3 = 5; }; }
try { S.f(); console.log("no throw", c3); } catch (e) { console.log("static-field", e.name, c3); }
function nested() {
  const n = 1;
  function h(f = () => { n = 2; }) { f(); }
  try { h(); console.log("no throw", n); } catch (e) { console.log("default-nested", e.name, n); }
}
nested();

var gen4 = function* gen() {
  class C { m() { gen = 1; } }
  try { new C().m(); console.log("no throw", typeof gen); } catch (e) { console.log("gen-class-method", e.name, typeof gen); }
  yield 0;
};
gen4().next();
var gen5 = function* gen() {
  class C { m() { var gen; gen = 1; return gen; } }
  console.log("gen-class-shadow", new C().m(), typeof gen);
  yield 0;
};
gen5().next();
var gen6 = function* gen() {
  (() => { gen = 1; })();
  console.log("gen-sloppy-arrow", typeof gen);
  yield 0;
};
gen6().next();
