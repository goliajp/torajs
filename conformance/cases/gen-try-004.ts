// nested try regions: inner catch rethrow escalates to outer catch
function* g(): Generator<any> {
  try {
    try {
      yield "inner";
      throw new Error("x");
    } catch (a) {
      yield "caught-inner";
      throw new Error("y");
    }
  } catch (b) {
    yield "caught-outer:" + (b as Error).message;
  }
  yield "end";
}
const it = g();
let r = it.next();
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
console.log(it.next().done);
