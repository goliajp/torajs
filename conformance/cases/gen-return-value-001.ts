// t1 — sync: return value becomes the done step, not an iteration
function* g1() { yield 5; return 1; }
var it = g1();
console.log(JSON.stringify(it.next()));
console.log(JSON.stringify(it.next()));
console.log(JSON.stringify(it.next()));

// t2 — for-of skips the return value (done step is not iterated)
for (const x of g1()) console.log(x);

// t3 — bare return; completes with undefined value
function* g3() { yield 7; return; yield 8; }
var it3 = g3();
console.log(JSON.stringify(it3.next()));
console.log(JSON.stringify(it3.next()));

// t4 — conditional return inside the body
function* g4(n: number) {
  if (n > 0) { return "pos"; }
  yield "neg";
}
console.log(JSON.stringify(g4(1).next()));
console.log(JSON.stringify(g4(-1).next()));

// t5 — async generator return value
async function* ag() { yield 10; return 20; }
async function main() {
  var a = ag();
  console.log(JSON.stringify(await a.next()));
  console.log(JSON.stringify(await a.next()));
  console.log(JSON.stringify(await a.next()));
}
main();
