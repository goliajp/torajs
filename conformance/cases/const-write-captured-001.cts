// A write to a `const` declared in a function body, a block, a try /
// finally list, a switch clause or a method body, made from a nested
// function or arrow, is a TypeError (§9.1.1.1.5) after the rhs is
// evaluated (§13.15.2), and the binding keeps its value — for plain,
// compound, update, destructuring and for-in/of-head writes. A write
// that reaches the binding in its dead zone is a ReferenceError
// (SetMutableBinding step 3); `o++` calls valueOf before it throws. A
// scope that rebinds the name owns its writes.
//
// Oracle is node's script goal, pinned in `.expected`: bun's
// transpiler refuses the assignment statically.
function f(a) {
  const b = a;
  try { b = 1 + a; } catch (e) { console.log("same", e.name, b) }
  function g() { b = 7; }
  try { g(); console.log("no throw", b) } catch (e) { console.log("fn", e.name, b) }
  try { (() => { b = 1 + a; })(); console.log("no throw", b) } catch (e) { console.log("arrow", e.name, b) }
  let n = 0;
  try { (() => { b = (n++, 5); })(); console.log("no throw", b) } catch (e) { console.log("rhs-first", e.name, n, b) }
  try { (() => { b += 1; })(); console.log("no throw", b) } catch (e) { console.log("compound", e.name, b) }
  try { (() => { b++; })(); console.log("no throw", b) } catch (e) { console.log("postinc", e.name, b) }
  try { (() => { ++b; })(); console.log("no throw", b) } catch (e) { console.log("preinc", e.name, b) }
  try { (() => { [b] = [3]; })(); console.log("no throw", b) } catch (e) { console.log("dstr-arr", e.name, b) }
  try { (() => { ({ b } = { b: 4 }); })(); console.log("no throw", b) } catch (e) { console.log("dstr-obj", e.name, b) }
  try { (() => { for (b of [8]) {} })(); console.log("no throw", b) } catch (e) { console.log("forof", e.name, b) }
  try { (() => { for (b in { k: 1 }) {} })(); console.log("no throw", b) } catch (e) { console.log("forin", e.name, b) }
  try { (function () { b = 9; })(); console.log("no throw", b) } catch (e) { console.log("fnexpr", e.name, b) }
  const h = () => { let b = 1; b = 2; return b; };
  console.log("shadow", h(), b);
  { const c = 1; try { (() => { c = 2; })(); console.log("no throw", c) } catch (e) { console.log("block", e.name, c) } }
}
f(42);
const t = 1;
function outer() { (() => { t = 2; })(); }
try { outer(); } catch (e) { console.log("top", e.name, t) }

function sw(k) {
  switch (k) {
    case 1:
      const s = "one";
    // fall through
    default:
      try { (() => { s = "two"; })(); console.log("no throw"); } catch (e) { console.log("switch", e.name, s); }
  }
}
sw(1);
function tf() {
  try {
    const t = 3;
    try { (() => { t = 4; })(); } catch (e) { console.log("try", e.name, t); }
  } finally {
    const u = 5;
    try { (() => { u = 6; })(); } catch (e) { console.log("finally", e.name, u); }
  }
}
tf();
class K {
  m() {
    const q = 7;
    try { (() => { q = 8; })(); } catch (e) { console.log("method", e.name, q); }
    return q;
  }
}
console.log(new K().m());
function shadows() {
  const v = 1;
  function g() { var v = 2; v = 3; return v; }
  const h = (v) => { v = 4; return v; };
  function k() { try { throw 0; } catch (v) { v = 5; return v; } }
  function l() { let r = 0; for (let v = 0; v < 2; v = v + 1) { r = r + v; } return r; }
  console.log("shadows", g(), h(0), k(), l(), v);
}
shadows();
function* gen() {
  const w = 1;
  const f = () => { w = 2; };
  try { f(); } catch (e) { yield e.name + " " + w; }
}
console.log("gen", gen().next().value);
async function as() {
  const x = 1;
  await null;
  try { (() => { x = 2; })(); } catch (e) { console.log("async", e.name, x); }
}
as();
function reads() {
  const base = 10;
  const add = (n) => base + n;
  let acc = 0;
  const bump = () => { acc = acc + base; };
  bump(); bump();
  console.log("reads", add(5), acc);
}
reads();

function tdz() {
  const early = () => { late = 2; };
  try { early(); } catch (e) { console.log("tdz-assign", e.name); }
  const earlyInc = () => { late++; };
  try { earlyInc(); } catch (e) { console.log("tdz-inc", e.name); }
  const late = 1;
  try { early(); } catch (e) { console.log("after", e.name, late); }
}
tdz();
function inc() {
  let calls = 0;
  const o = { valueOf() { calls++; return 1; } };
  try { (() => { o++; })(); } catch (e) { console.log("inc", e.name, calls, typeof o); }
}
inc();
function topEarly() { (() => { top2 = 3; })(); }
try { topEarly(); } catch (e) { console.log("top-tdz", e.name); }
const top2 = 2;
