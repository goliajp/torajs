// A generator or async local holding the result of a generic call, or
// a closure that returns one, keeps it across a `yield` / `await`: the
// callee's `T` is settled per call, not a type the machine's field can
// name.

function id<T>(x: T): T { return x; }
function pair<T>(x: T): T[] { return [x, x]; }

function* direct(n: number) {
  const a = id(n);
  const b = id("s" + n);
  const p = pair(n);
  yield 0;
  yield a + 1;
  yield b.length;
  yield p.length + p[1];
}
for (const v of direct(7)) console.log("direct", v);

function* viaClosure() {
  let n = 1;
  const k = function () { return id(n); };
  yield 0;
  n = 2;
  yield k();
}
for (const v of viaClosure()) console.log("viaClosure", v);

async function asyncClosure() {
  let n = 1;
  const k = function () { return id(n); };
  await null;
  n = 3;
  return k();
}
asyncClosure().then(v => console.log("asyncClosure", v));
