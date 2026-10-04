// A guard on a module binding proves nothing inside a function that
// can run again between the guard and the negation: a recursive call,
// another generator instance across a `yield`, another async call
// across an `await` all run the same body's `x = 0`. Each `-x` here
// has to answer -0. A function's own `let` is a fresh binding per call
// and keeps its proof.
function show(label: string, v: number): void {
  console.log(label, 1 / v, Object.is(v, -0));
}

let x = 1;
function f(reset: boolean): number {
  if (reset) {
    x = 0;
    return 0;
  }
  if (x !== 0) {
    f(true);
    return -x;
  }
  return 1;
}
show("recursion", f(false));

let y = 1;
function* gen(reset: boolean): Generator<number> {
  if (reset) {
    y = 0;
    return;
  }
  if (y !== 0) {
    yield 1;
    yield -y;
  }
}
const it = gen(false);
it.next();
gen(true).next();
show("yield", it.next().value as number);

let z = 1;
async function af(reset: boolean): Promise<number> {
  if (reset) {
    z = 0;
    return 0;
  }
  if (z !== 0) {
    await null;
    return -z;
  }
  return 1;
}
const p = af(false);
af(true);
p.then((v) => show("await", v));

function own(k: number): number {
  let w = k;
  if (w !== 0) {
    if (k > 0) own(k - 1);
    return -w;
  }
  return 0;
}
show("own", own(3));
