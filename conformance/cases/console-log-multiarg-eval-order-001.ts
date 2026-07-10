// Chunk 808 — multi-arg console.log evaluates ALL arguments before
// printing (ES argument-evaluation order). The old streaming loop
// interleaved a side-effecting argument's output into the log line:
// `console.log(1, s("a"))` printed "1 a\n9" where bun prints
// "a\n1 9". Phase-1 rc_inc also pins print-as-evaluated semantics —
// a later argument reassigning a binding must not change what an
// earlier argument prints.

// side-effecting later arg prints its output BEFORE the log line
function s(m: string) { console.log(m); return 9 }
console.log(1, s("a"));

// void-call arg: effect first, undefined in the joined line
function v(m: string) { console.log(m) }
console.log(1, v("b"));

// reassignment between args: earlier arg prints the OLD value
let x = "old";
function mut() { x = "new"; return 0 }
console.log(x, mut(), x);

// mixed primitive row
console.log("a", 1, true, null, undefined, 2.5);

// typed array arg stays live across a later call arg
const xs = [1, 2];
function mut2() { return 7 }
console.log(xs, mut2());

// borrowed string stays live across a later call arg
const s2 = "live";
function f2() { return 5 }
console.log(s2, f2(), s2.length);
