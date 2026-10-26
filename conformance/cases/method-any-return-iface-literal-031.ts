// A method read as a value through an interface and stored into a Set,
// or as the key of a Map keyed any or Function, that the program then
// iterates: the collection hands the function back as an any value,
// and an any value written into a slot typed `() => number` (an array
// element, a callback's parameter, a binding, an object type's member)
// is called through that return, so the interface keeps it, as for a
// Map key typed `() => number`. The values are integral: an answer kept
// typed `number` that is not an integer reads truncated, a defect of
// the kept return's width that this case does not cover.
interface A {
  m(): number;
}
interface B {
  m(): number;
}
interface C {
  m(): number;
}
interface D {
  m(): number;
}
class SA implements A {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class SB implements B {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class SC implements C {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class SD implements D {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function viaFrom(raw: any) {
  const a: A = { m: () => raw.y };
  const st = new Set<() => number>();
  st.add(a.m);
  const fs: (() => number)[] = Array.from(st);
  console.log(fs[0]() + 1, new SA(raw).m() + 1);
}
function viaForEach(raw: any) {
  const b: B = { m: () => raw.y };
  const st = new Set<() => number>();
  st.add(b.m);
  st.forEach((k: () => number) => console.log(k() + 1));
  console.log(new SB(raw).m() + 1);
}
function viaAnyKey(raw: any) {
  const c: C = { m: () => raw.y };
  const ma = new Map<any, string>();
  ma.set(c.m, "a");
  ma.forEach((v: string, k: any) => {
    const g: () => number = k;
    console.log(v, g() + 1);
  });
  console.log(new SC(raw).m() + 1);
}
function viaFunctionKey(raw: any) {
  const d: D = { m: () => raw.y };
  const mf = new Map<Function, string>();
  mf.set(d.m, "f");
  for (const k of mf.keys()) {
    const o: { g: () => number } = { g: k as any };
    console.log(o.g() + 1);
  }
  console.log(new SD(raw).m() + 1);
}
const raw = JSON.parse('{"x":2,"y":3}');
viaFrom(raw);
viaForEach(raw);
viaAnyKey(raw);
viaFunctionKey(raw);
