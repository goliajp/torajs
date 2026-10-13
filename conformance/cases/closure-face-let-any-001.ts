// A fn-typed binding that takes an `any` value, by its initializer or a
// store, calls the value by the value's own convention and answers what
// it returns, not the annotation's type.
function mkS(k: number): (y?: number) => any { return (y?: number) => "s" + k }
function mkN(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) + k }
const raw: any = mkS(1);
const rawN: any = mkN(10);

const c: (y?: number) => number = raw;
const w = c(2);
console.log(typeof w + ":" + w);

let m: (y?: number) => number = raw;
const v = m();
console.log(typeof v + ":" + v);
m = rawN;
console.log(m(), m(3));

let n: (y?: number) => number = (y?: number) => 3;
console.log(n());
n = raw;
const u = n();
console.log(typeof u + ":" + u);

function local(r: any) {
  const g: (y?: number) => number = r;
  let h: (y?: number) => number = (y?: number) => 1;
  h = r;
  const a = g(), b = h(4);
  return typeof a + ":" + a + " " + typeof b + ":" + b;
}
console.log(local(raw), local(rawN));

function viaGlobal() {
  const b = n();
  return typeof b + ":" + b;
}
console.log(viaGlobal());

// a call checked before the store that hands the binding an `any` value
function loop() {
  let p: (y?: number) => number = (y?: number) => 2;
  const out: string[] = [];
  for (let i = 0; i < 2; i++) {
    const r = p();
    out.push(typeof r + ":" + r);
    p = raw;
  }
  return out.join(" ");
}
console.log(loop());
let q: (y?: number) => number = (y?: number) => 2;
const callQ = () => { const r = q(); return typeof r + ":" + r };
console.log(callQ());
q = raw;
console.log(callQ());

// a slot that passes nothing and discards the return
let fire: () => void = () => {};
fire = (() => console.log("fired")) as any;
fire();

// a typed binding keeps its direct call
const t: (y: number) => number = (y: number) => y * 2;
console.log(t(21));
