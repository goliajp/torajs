// 546-03 — a promoted (un-annotated, named-fn-read) method-bearing
// literal whose fields include a nested literal: the this-chain read
// inside the method must dispatch through the any lane, not
// struct-offset against the dynobj receiver (the box-bits hole).
const m = { box: { d: 1 }, read() { return this.box.d + 10; } };
function pull() { console.log(m.read()); }
pull();

// alias hop inside the body
const n = { box: { d: 2 }, grab() { const t = this.box; return t.d; } };
function pull2() { console.log(n.grab()); }
pull2();

// write through the this-chain, state observed across named fns
const w = { box: { d: 1 }, bump() { this.box.d = this.box.d + 5; return this.box.d; } };
function goA() { console.log(w.bump()); }
function goB() { console.log(w.bump()); }
goA(); goB();
console.log(w.box.d);

// identity through this
const idy = { w: null, me() { return this; } };
function idp() { console.log(idy.me() === idy); }
idp();

// closure + null + method mix (the __inlobj-refused profile)
const mix = { v: () => 42, w: null, read() { return this.w; } };
function mixp() { console.log(mix.read()); console.log(mix.v()); }
mixp();

// method calling a sibling method plus a nested read
const mm = { a: 1, box: { q: 9 }, get2() { return this.a + 1; }, get3() { return this.get2() + this.box.q; } };
function mmp() { console.log(mm.get3()); }
mmp();

// both homes agree: top-level call and named-fn call
const dual = { box: { d: 3 }, read() { return this.box.d; } };
console.log(dual.read());
function dualp() { console.log(dual.read()); }
dualp();

// non-numeric nested field through the chain
const s = { box: { s: "hi" }, read() { return this.box.s + "!"; } };
function sp() { console.log(s.read()); }
sp();
