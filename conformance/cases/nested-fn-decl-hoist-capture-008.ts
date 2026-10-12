// A generator body's declaration lifted to top level is renamed only
// where its name is still the one in scope: a block, `catch` clause,
// loop head, `switch`, parameter, `var` or class member of the same
// name keeps reading its own binding, after a `yield` as before it.

function* inBlocks() {
  function f() { return 1; }
  yield 0;
  { const f = () => 9; yield f(); }
  try { throw 5; } catch (f) { yield f; }
  for (const f of [11, 12]) { yield f; }
  for (let f = 30; f < 32; f++) { yield f; }
  switch (2) { case 2: const f = () => 33; yield f(); }
  yield f();
}
for (const v of inBlocks()) console.log(v);

function* inFunctions() {
  function f() { return 1; }
  function g() { { const f = () => 2; return f(); } }
  yield 0;
  function k() { { const f = () => 8; return f(); } }
  function kv() { var f = 50; return f; }
  const a = (f: number) => f + 1;
  const b = () => { if (true) { var f = 60; } return f; };
  const c = () => { { const f = () => 13; return f(); } };
  yield g();
  yield k();
  yield kv();
  yield a(40);
  yield b();
  yield c();
  yield f();
}
for (const v of inFunctions()) console.log(v);

function* inClasses() {
  function f() { return 21; }
  yield 0;
  class C { get() { return f(); } }
  class D {
    static s = f() + 70;
    static { console.log("static", f()); }
    m(f: number) { return f; }
  }
  yield new C().get();
  yield D.s;
  yield new D().m(71);
}
for (const v of inClasses()) console.log(v);
