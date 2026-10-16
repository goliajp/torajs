// The scopes that own a write to a name an outer `const` also names,
// and the writes to an outer `const` those scopes still let through.
// A `let` / `const` / class / function in a `try`, `catch` or
// `finally` list, in a switch clause, or a destructured catch
// parameter binds its own name, and so does a named function
// expression's own name (§15.2.5; a sloppy write to it is ignored).
// Writes to the outer `const` from a `try` body or `finally` whose
// catch parameter shadows it, from a class method, from inside a
// `for (const …;;)` loop body (one name or several) or a
// `for (const x of …)` body, are TypeErrors (§9.1.1.1.5).
//
// Oracle is node's script goal, pinned in `.expected`: bun's
// transpiler refuses the assignment statically.
function owns(k) {
  const x = 1;
  function inTry() { try { let x = 0; x = 2; return x; } catch {} }
  function inCatch() { try { throw 0; } catch { let x = 0; x = 3; return x; } }
  function inFinally() { try {} finally { let x = 0; x = 4; return x; } }
  function inSwitch() { switch (k) { case 1: let x = 0; x = 5; return x; } }
  function inSwitchTdz() { switch (k) { case 0: let x = 0; case 1: try { x = 6; } catch (e) { return e.name; } } }
  function inDstrCatch() { try { throw { x: 0 }; } catch ({ x }) { x = 7; return x; } }
  function inClassTry() { try { class x {} x = 8; return x; } catch {} }
  console.log("owns", inTry(), inCatch(), inFinally(), inSwitch(), inSwitchTdz(), inDstrCatch(), inClassTry(), x);
}
owns(1);
const b = 1;
(function b() { b = 2; })();
const fe = function b() { b = 3; return typeof b; };
console.log("fnexpr-self", b, fe());
function leaks() {
  const y = 1;
  function g() {
    try { y = 2; } catch (y) { y = y; throw y; }
  }
  try { g(); console.log("no throw", y); } catch (e) { console.log("catch-param-try", e.name, y); }
  function h() {
    try { throw 0; } catch (y) { y = 0; } finally { y = 3; }
  }
  try { h(); console.log("no throw", y); } catch (e) { console.log("catch-param-finally", e.name, y); }
  class C { m() { y = 4; } static s() { y = 5; } }
  try { new C().m(); console.log("no throw", y); } catch (e) { console.log("method", e.name, y); }
  try { C.s(); console.log("no throw", y); } catch (e) { console.log("static", e.name, y); }
  for (const i = 0; ; ) {
    try { (() => { i = 1; })(); console.log("no throw", i); } catch (e) { console.log("for-head", e.name, i); }
    break;
  }
  for (const a = 0, c = 9; ; ) {
    try { (() => { c = a; })(); console.log("no throw", c); } catch (e) { console.log("for-head-2", e.name, c); }
    break;
  }
  for (const v of [7]) {
    try { (() => { v = 1; })(); console.log("no throw", v); } catch (e) { console.log("for-of", e.name, v); }
  }
  for (const { w } of [{ w: 8 }]) {
    try { (() => { w = 1; })(); console.log("no throw", w); } catch (e) { console.log("for-of-dstr", e.name, w); }
  }
}
leaks();
