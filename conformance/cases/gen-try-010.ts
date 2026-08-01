// try/catch/finally combined: catch handles first, finally always runs
// (§14.13.3 nesting order); a rethrow from catch still walks finally
let log: string[] = [];
function* g(mode: number): Generator<any> {
  try {
    yield "b1";
    if (mode >= 1) {
      throw new Error("boom");
    }
    yield "b2";
  } catch (e) {
    yield "c:" + (e as Error).message;
    if (mode === 2) {
      throw new Error("re");
    }
  } finally {
    log.push("fin" + mode);
    yield "f";
  }
  yield "after";
}
// mode 0: no throw — B then F
const a = g(0);
let r = a.next();
while (!r.done) { console.log(r.value); r = a.next(); }
console.log(log.join(","));
// mode 1: B throws — C then F
const b = g(1);
r = b.next();
while (!r.done) { console.log(r.value); r = b.next(); }
console.log(log.join(","));
// mode 2: C rethrows — F runs, then the rethrow escapes
const c = g(2);
console.log(c.next().value);
console.log(c.next().value);
console.log(c.next().value);
try {
  c.next();
} catch (e) {
  console.log("escaped:" + (e as Error).message);
}
console.log(log.join(","));
console.log(c.next().done);
