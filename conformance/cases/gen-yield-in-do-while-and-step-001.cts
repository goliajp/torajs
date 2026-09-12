// Rotation 636 — a `yield` in a do-while CONDITION or a for STEP:
// positions a `continue` must reach, so the parser rotates the loop
// (`for (let f = true;;) { if (!f) { <step / cond> } f = false; … }`)
// instead of moving the yield to the body top. The first pass skips
// the head; every later pass, `continue` included, runs the step and
// re-evaluates the condition in source order. Labeled continue from
// a nested loop, `break`, and `yield*` ride along.
// --- d1: do-while, cond yields per pass, continue reaches it
function* g1() { var n = 0; do { n++; if (n === 2) continue; n += 10; } while (yield n); console.log("done", n); }
var it1 = g1(); console.log(it1.next().value); console.log(it1.next(true).value); console.log(it1.next(true).value); it1.next(false);
// --- d2: for step yields, cond plain, continue reaches the step
function* g2() { for (var i = 0; i < 3; i = yield i) { if (i === 1) continue; console.log("body", i); } console.log("end", i); }
var it2 = g2(); console.log(it2.next().value); console.log(it2.next(1).value); console.log(it2.next(2).value); it2.next(3);
// --- d3: both cond and step yield; order is step then cond
function* g3() { for (var i = 0; yield "c" + i; yield "s" + i) { i++; console.log("body", i); } console.log("end", i); }
var it3 = g3(); console.log(it3.next().value); console.log(it3.next(true).value); console.log(it3.next().value); console.log(it3.next(true).value); console.log(it3.next().value); it3.next(false);
// --- d4: labeled continue into a do-while from a nested loop; break leaves
function* g4() {
  var n = 0;
  outer: do {
    n++;
    for (var j = 0; j < 3; j++) { if (j === 1) continue outer; console.log("nj", n, j); }
    console.log("never");
  } while (yield n);
  console.log("out", n);
  var k = 0;
  do { k++; if (k === 3) break; } while (yield* [1, 2]);
  console.log("k", k);
}
var it4 = g4(); console.log(it4.next().value); console.log(it4.next(true).value); it4.next(false); console.log(it4.next().value); console.log(it4.next().value); console.log(it4.next(true).value); console.log(it4.next().value); it4.next(true);
// --- d5: `let` in the for init next to the rotated step
function* g5() { for (let i = 0; i < 2; i = yield i) { console.log("li", i); } console.log("lend"); }
var it5 = g5(); console.log(it5.next().value); console.log(it5.next(1).value); it5.next(2);
