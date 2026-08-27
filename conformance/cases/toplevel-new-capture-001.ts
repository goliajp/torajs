// A top-level binding initialized with `new C()` had no home a named
// function body could see: the any-promotion verdict refuses class
// instances on purpose (boxing one demotes main-side method calls to
// the any-lane), and nothing else claimed them — so every named-fn
// read answered "unknown identifier". It now promotes under its own
// class spelling, the lane a written `let e: C = new C()` already
// rides.
class Box { constructor(v) { this.v = v } get() { return this.v } }
let boxed = new Box(3);
function readField() { return boxed.v }
function callMethod() { return boxed.get() }
console.log(readField(), callMethod(), boxed.get());

let err = new Error("boom");
function readMessage() { return err.message }
function readIdentity() { return readMessage() === "boom" }
console.log(readMessage(), readIdentity());

let terr = new TypeError("te");
function readTypeError() { return terr.message }
console.log(readTypeError());

// The same binding through a generator body and an async body — both
// are named fns with no capture machinery of their own.
let g0 = new Box(7);
function* gen() { yield g0.v }
console.log([...gen()].join(","));

async function afn() { return g0.get() }
afn().then(v => console.log("async", v));

// Reassignment still lands on the one home.
let cur = new Box(1);
function bump() { cur = new Box(cur.v + 1) }
bump();
bump();
console.log(cur.v);
