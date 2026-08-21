// rotation 470 — the four inlined guards on the class hot path keep
// their slow halves: frozen-field write still throws, a WeakRef'd
// instance still notifies the registry on death, an expando cycle on
// an all-scalar class still gets buffered (and unbuffered on drop).
class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

// frozen write → TypeError through the inline-guard slow path
const frozen = new P(1);
Object.freeze(frozen);
try {
  frozen.x = 9;
  console.log("no throw");
} catch (e) {
  console.log("caught", frozen.x);
}

// WeakRef observer: target dying must clear the ref
let target: any = new P(2);
const wr = new WeakRef(target);
console.log(typeof wr.deref());
target = null;

// expando cycle between all-scalar-field instances: the buffer gate
// must still fire (props non-NULL), and program exit must stay clean
for (let i = 0; i < 100; i = i + 1) {
  let a: any = new P(i);
  let b: any = new P(i + 1);
  a.peer = b;
  b.peer = a;
  a = null;
  b = null;
}

// churn after the cycles so a dangling buffer entry would surface
let acc = 0;
for (let i = 0; i < 1000; i = i + 1) {
  const p = new P(i);
  acc = acc + p.x;
}
console.log(acc);
