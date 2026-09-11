// Rotation 635 — a `yield` in a while / for CONDITION: the parser moves
// the hoisted yield to the top of the loop body and empties the
// clause, so it runs (and yields) once per iteration; `continue`
// reaches it, labeled continue too, `yield*` as well. do-while and the
// for step stay refused.
// --- y2
function* g1() { var n = 0; while (yield n) { n++; } console.log("done", n); }
var it1 = g1(); it1.next(); it1.next(true); it1.next(true); it1.next(false);
// --- y5
function* g2() { for (var i = 0; yield i; i++) { console.log("body", i); } console.log("end", i); }
var it2 = g2(); console.log(it2.next().value); console.log(it2.next(true).value); console.log(it2.next(true).value); it2.next(false);
// --- y6
function* g3() { var n = 0; while (yield n) { n++; if (n === 2) continue; n += 10; } console.log("done", n); }
var it3 = g3(); it3.next(); it3.next(true); it3.next(true); it3.next(true); it3.next(false);
// --- y7
function* g4() {
  outer: for (var i = 0; yield i; i++) {
    for (var j = 0; j < 3; j++) { if (j === 1) continue outer; console.log("ij", i, j); }
  }
  console.log("end", i);
  var k = 0; while (yield* [1, 2]) { k++; }
  console.log("k", k);
}
var it4 = g4(); it4.next(); it4.next(true); it4.next(true); it4.next(false); it4.next(); it4.next(); it4.next(true); it4.next(false);
