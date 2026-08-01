// yield inside try body — from-try.js shape, normal drive + rethrow catch
function* g(): Generator<number> {
  try {
    yield 1;
    yield 2;
  } catch (err) {
    throw err;
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
