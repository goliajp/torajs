// iter.throw() while suspended inside try-finally: finally runs, then
// the injected error escapes (throw-suspendedYield-try-finally-throw
// shape); a return inside finally overrides the completion
let log: string[] = [];
function* g(): Generator<any> {
  try {
    yield 1;
    yield 99;
  } finally {
    log.push("fin");
  }
}
const it = g();
console.log(it.next().value);
try {
  it.throw(new Error("inj"));
} catch (e) {
  console.log("escaped:" + (e as Error).message);
}
console.log(log.join(","));
console.log(it.next().done);

function* h(): Generator<any> {
  try {
    yield 1;
  } finally {
    return 34;
  }
}
const it2 = h();
console.log(it2.next().value);
const r = it2.next();
console.log(r.value);
console.log(r.done);
