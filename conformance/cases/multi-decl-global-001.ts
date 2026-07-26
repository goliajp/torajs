// Rotation 230 刀 6 (RFC 20260727-dstr-assignment) — a
// multi-declarator top-level let registers as a K.3 data global like
// a flat one. Multi-declarators parse into Stmt::Multi, and both
// global-registration walks (checker pass_2 pre-pass /
// collect_toplevel_globals) iterated the top-level vec flat, so
// `let v, w; function f() { v = 1; }` answered "assignment to
// undeclared v". This is the test262 dstr-assignment preamble shape
// (`let v2, vNull, vHole, vUndefined, vOob;` + writes in an async fn).

// sync fn writes into multi-declared bindings
let v, w;
function f() {
  v = 1;
  w = "s";
}
f();
console.log(v, w); // 1 s

// async fn + the 654 preamble-and-pattern shape end to end
let v2, vNull, vHole, vUndefined, vOob;
let iterCount = 0;
async function main() {
  for await ([v2 = 10, vNull = 11, vHole = 12, vUndefined = 13, vOob = 14] of [[2, null, , undefined]]) {
    console.log(v2, vNull, vHole, vUndefined, vOob); // 2 null 12 13 14
    iterCount += 1;
  }
  console.log(iterCount); // 1
}
main();
