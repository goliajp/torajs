// ES §13.3.6.1 — beyond-arity calls are universally admitted: every
// argument evaluates (side effects kept, in order), extras never bind
// a parameter, and argc-carrying ABIs pass the real count. Replaces
// the name-keyed length-only closure wedge (RFC
// 20260810-indirect-argc-abi S3.5).

// named fn direct call
function f1() { return 1; }
console.log(f1(1, 2, 3));

// plain closure
const f2 = () => 1;
console.log(f2(1, 2, 3));

// length-only tier still counts the extras through the hidden argc
const f3 = function () { return arguments.length; };
console.log(f3(1, 2, 3));

// argv-face tier sees the extras' values
const f4 = function (a: number) { return arguments[1]; };
console.log(f4(1, 42));

// side effects evaluate in order, values dropped
function f5() { return 0; }
let s = "";
f5((s += "a", 1), (s += "b", 2));
console.log(s);

// callback param called beyond its face arity
function h(cb: (x: number) => number) { return cb(5, 9); }
console.log(h((x: number) => x));

// object-literal method (checker used to loud-reject this shape)
const o = { n: 3, m(x: number) { return this.n + x; } };
console.log(o.m(1, 2));

// method-call side-effect order
const p = { m(x: number) { return x; } };
let t = "";
p.m((t += "a", 1), (t += "b", 2));
console.log(t);

// fn-sig struct field (bare fn-ptr slot)
type S = { f: (x: number) => number };
function mk(): S { return { f: (x: number) => x * 3 }; }
const sv = mk();
console.log(sv.f(2, 8));

// fresh-owned extra releases without a callee borrow
console.log(p.m(1, "extra".repeat(2)));
