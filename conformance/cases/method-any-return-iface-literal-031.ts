// A method read as a value through an interface and stored into a Set,
// or as the key of a Map keyed any or Function, that the program then
// iterates: the collection hands the function back as an any value,
// and an any value written into a slot typed `() => number` (an array
// element, a callback's parameter, a binding, an object type's member)
// is called through that return, so the interface keeps it, as for a
// Map key typed `() => number`. The collection may be reached through
// the answer of `add` / `set` (which is the collection), another name,
// a parameter, or a closure. Every interface names its method on its
// own: interfaces of one shape keep or retype the return together, so
// one store that keeps it would hide another that does not; and every
// function names its collection on its own, as a collection's reads
// are matched by name. The values
// are integral: an answer kept typed `number` that is not an integer
// reads truncated, a defect of the kept return's width that this case
// does not cover.
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
interface D {
  mD(): number;
}
class SD implements D {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mD(): number {
    return this.v.x;
  }
}
interface E {
  mE(): number;
}
class SE implements E {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mE(): number {
    return this.v.x;
  }
}
interface F {
  mF(): number;
}
class SF implements F {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mF(): number {
    return this.v.x;
  }
}
interface G {
  mG(): number;
}
class SG implements G {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mG(): number {
    return this.v.x;
  }
}
interface H {
  mH(): number;
}
class SH implements H {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mH(): number {
    return this.v.x;
  }
}
interface I {
  mI(): number;
}
class SI implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  mI(): number {
    return this.v.x;
  }
}
function viaFrom(raw: any) {
  const a: A = { mA: () => raw.y };
  const st0 = new Set<() => number>();
  st0.add(a.mA);
  const fs: (() => number)[] = Array.from(st0);
  console.log(fs[0]() + 1, new SA(raw).mA() + 1);
}
function viaForEach(raw: any) {
  const b: B = { mB: () => raw.y };
  const st1 = new Set<() => number>();
  st1.add(b.mB);
  st1.forEach((k: () => number) => console.log(k() + 1));
  console.log(new SB(raw).mB() + 1);
}
function viaAnyKey(raw: any) {
  const c: C = { mC: () => raw.y };
  const ma2 = new Map<any, string>();
  ma2.set(c.mC, "a");
  ma2.forEach((v: string, k: any) => {
    const g: () => number = k;
    console.log(v, g() + 1);
  });
  console.log(new SC(raw).mC() + 1);
}
function viaFunctionKey(raw: any) {
  const d: D = { mD: () => raw.y };
  const mf3 = new Map<Function, string>();
  mf3.set(d.mD, "f");
  for (const k of mf3.keys()) {
    const o: { g: () => number } = { g: k as any };
    console.log(o.g() + 1);
  }
  console.log(new SD(raw).mD() + 1);
}
function viaAddAnswer(raw: any) {
  const e: E = { mE: () => raw.y };
  const st4 = new Set<() => number>();
  const fs: (() => number)[] = Array.from(st4.add(e.mE));
  console.log(fs[0]() + 1, new SE(raw).mE() + 1);
}
function viaSetAnswer(raw: any) {
  const f: F = { mF: () => raw.y };
  const ma5 = new Map<any, string>();
  ma5.set(f.mF, "s").forEach((v: string, k: any) => {
    const g: () => number = k;
    console.log(v, g() + 1);
  });
  console.log(new SF(raw).mF() + 1);
}
function viaOtherName(raw: any) {
  const g: G = { mG: () => raw.y };
  const all6 = new Set<() => number>();
  const st6 = all6;
  st6.add(g.mG);
  const fs: (() => number)[] = Array.from(all6);
  console.log(fs[0]() + 1, new SG(raw).mG() + 1);
}
function put(st7: Set<any>, o: H) {
  st7.add(o.mH);
}
function viaParameter(raw: any) {
  const h: H = { mH: () => raw.y };
  const all7 = new Set<() => number>();
  put(all7, h);
  const fs: (() => number)[] = Array.from(all7);
  console.log(fs[0]() + 1, new SH(raw).mH() + 1);
}
function viaClosure(raw: any) {
  const i: I = { mI: () => raw.y };
  const st8 = new Set<() => number>();
  st8.add(i.mI);
  const first = () => {
    const fs: (() => number)[] = Array.from(st8);
    return fs[0]() + 1;
  };
  console.log(first(), new SI(raw).mI() + 1);
}
const raw = JSON.parse('{"x":2,"y":3}');
viaFrom(raw);
viaForEach(raw);
viaAnyKey(raw);
viaFunctionKey(raw);
viaAddAnswer(raw);
viaSetAnswer(raw);
viaOtherName(raw);
viaParameter(raw);
viaClosure(raw);
