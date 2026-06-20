// S277 — Map/Set.{keys,values,entries}(...trailing) per ES
// §{23,24}.X iterator-factory trailing-arg ignore. Spec defines
// 0-arg only; trailing slots silent-drop. tora's static sig was
// `Vec::new() → MapIter` (strict 0-arg), so 1+ args bounced.
//
// fixture verifies trailing exprs eval-and-drop via step counter;
// uses for-of loop over iterators (spread / `.next()` chain are
// pre-existing substrate gaps — not covered here).

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

const m = new Map<string, number>([["a", 1], ["b", 2]]);
let kbuf = "";
for (const k of m.keys(step("a"))) {
  kbuf = kbuf + k;
}
console.log(kbuf);

let vsum = 0;
for (const v of m.values(step("b"), step("c"))) {
  vsum = vsum + v;
}
console.log(vsum);

const s = new Set([10, 20]);
let ssum = 0;
for (const x of s.keys(step("d"))) {
  ssum = ssum + x;
}
console.log(ssum);

let ssum2 = 0;
for (const x of s.values(step("e"), step("f"), step("g"))) {
  ssum2 = ssum2 + x;
}
console.log(ssum2);

console.log("calls=" + calls);
