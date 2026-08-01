// try { yield } finally { cleanup } — normal path runs finally once;
// exception path runs finally then rethrows
let log: string[] = [];
function* g(fail: boolean): Generator<any> {
  try {
    yield 1;
    if (fail) {
      throw new Error("boom");
    }
    yield 2;
  } finally {
    log.push("fin");
  }
  yield 3;
}
const a = g(false);
console.log(a.next().value);
console.log(a.next().value);
console.log(a.next().value);
console.log(a.next().done);
console.log(log.join(","));
const b = g(true);
console.log(b.next().value);
try {
  b.next();
} catch (e) {
  console.log("caught:" + (e as Error).message);
}
console.log(log.join(","));
console.log(b.next().done);
