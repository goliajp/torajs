// A method read as a value through an interface and stored as the key
// of a Map keyed `() => any` that the program iterates: a key read back
// typed `() => any` is admitted by a slot typed `() => number` (a
// binding, an array element, a callback's parameter) and called through
// that return, so the interface keeps it, as for a Map keyed `any`.
// Every interface names its method, and every function its collection,
// on its own, so one store that keeps the return does not hide another.
// The values are integral: an answer kept typed `number` that is not
// an integer reads truncated, a defect of the kept return's width that
// this case does not cover.
interface A {
  mA(): number;
}
class SA implements A {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mA(): number {
    return this.v.x;
  }
}
interface B {
  mB(): number;
}
class SB implements B {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mB(): number {
    return this.v.x;
  }
}
interface C {
  mC(): number;
}
class SC implements C {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mC(): number {
    return this.v.x;
  }
}
function intoBinding(raw: any) {
  const a: A = { mA: () => raw.y };
  const mk0 = new Map<() => any, string>();
  mk0.set(a.mA, "b");
  for (const k of mk0.keys()) {
    const g: () => number = k;
    console.log(g() + 1);
  }
  console.log(new SA(raw).mA() + 1);
}
function intoElement(raw: any) {
  const b: B = { mB: () => raw.y };
  const mk1 = new Map<() => any, string>();
  mk1.set(b.mB, "e");
  const fs: (() => number)[] = [...mk1.keys()];
  console.log(fs[0]() + 1, new SB(raw).mB() + 1);
}
function intoCallback(raw: any) {
  const c: C = { mC: () => raw.y };
  const mk2 = new Map<() => any, string>();
  mk2.set(c.mC, "c");
  mk2.forEach((v: string, k: () => number) => console.log(v, k() + 1));
  console.log(new SC(raw).mC() + 1);
}
const raw = JSON.parse('{"x":2,"y":3}');
intoBinding(raw);
intoElement(raw);
intoCallback(raw);
