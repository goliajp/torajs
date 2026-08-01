// iter.throw() paused BEFORE the try region: escapes to the caller and
// the generator completes (try-catch-before-try shape); a throw on a
// suspended-in-catch state escapes too (within-catch shape)
let unreachable = 0;
function* g(): Generator<any> {
  yield 1;
  unreachable = unreachable + 1;
  try {
    yield 2;
  } catch (e) {
    yield "c:" + (e as Error).message;
    yield "c2";
  }
  yield 3;
}
const it = g();
console.log(it.next().value);
try {
  it.throw(new Error("early"));
} catch (e) {
  console.log("escaped:" + (e as Error).message);
}
console.log(unreachable);
console.log(it.next().done);

const it2 = g();
it2.next();
it2.next();
let r = it2.throw(new Error("mid"));
console.log(r.value); console.log(r.done);
try {
  it2.throw(new Error("incatch"));
} catch (e) {
  console.log("escaped2:" + (e as Error).message);
}
console.log(it2.next().done);
