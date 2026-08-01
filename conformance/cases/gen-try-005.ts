// iter.throw() while suspended at a yield INSIDE try: injected at the
// yield point, observed by the catch (try-catch-within-try shape)
function* g(): Generator<any> {
  try {
    yield 1;
    yield 99;
  } catch (e) {
    yield "caught:" + (e as Error).message;
  }
  yield "after";
}
const it = g();
let r = it.next();
console.log(r.value); console.log(r.done);
r = it.throw(new Error("inj"));
console.log(r.value); console.log(r.done);
r = it.next();
console.log(r.value); console.log(r.done);
console.log(it.next().done);
