// A function value of another face handed back through a return slot,
// called at once or through the binding that holds it; and a local of a
// generator or an async function, which the state machine keeps in a
// field.
const mk2 = (k: number) => (y?: number) => (y === undefined ? 5 : y) + k;
const mks = (k: number) => (y?: number): any => "s" + k;
type T = (y?: number) => number;

function ret(): T {
  return mk2(1);
}
console.log(ret()(), ret()(4));
const h = ret();
console.log(h(), h(4));
function viaGlobal(): number {
  return h(10);
}
console.log(viaGlobal());

function rets(): T {
  return mks(8);
}
console.log(rets()());
let hs = rets();
console.log(hs(), typeof hs());
hs = (y?: number) => 2;
console.log(hs());

function* gen(): Generator<number> {
  const g: T = mk2(1);
  yield 0;
  yield g();
  yield g(2);
}
for (const v of gen()) console.log(v);

async function af(): Promise<number> {
  const g: T = mk2(2);
  await 0;
  return g() + g(1);
}
af().then((v) => console.log(v));
