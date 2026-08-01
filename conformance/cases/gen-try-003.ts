// throw mid-try routes to catch; catch yields in a loop; let inside catch survives yields
function* g(): Generator<any> {
  let pre: number = 10;
  try {
    yield pre;
    throw new Error("mid");
    yield 99;
  } catch (e) {
    let i: number = 0;
    while (i < 2) {
      yield (e as Error).message + ":" + i;
      i = i + 1;
    }
  }
  yield "done";
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
