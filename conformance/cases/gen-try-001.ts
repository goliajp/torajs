// yield inside catch — from-catch.js shape + catch param read across yield
function* g(): Generator<any> {
  try {
    throw new Error("boom");
  } catch (err) {
    yield 1;
    yield (err as Error).message;
  }
  yield 3;
}
const it = g();
let r = it.next();
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
console.log(it.next().done);
