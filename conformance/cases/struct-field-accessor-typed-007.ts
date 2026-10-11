// A number field an accessor can reach holds any number: a getter's
// 0.5 and a redefined `value: 0.5` read back as 0.5, not truncated to
// an integer. Whole-object readers (JSON.stringify, Object.values /
// entries, Object.assign, a spread) see the getter's answer of the
// field's own type, nested or not. A call through a rest-parameter
// closure field whose getter answers a non-function throws TypeError.
class P { x: number; constructor() { this.x = 1; } }
const q = new P();
const r = new P();
Object.defineProperty(q, "x", { get() { return 0.5; } });
console.log(q.x, r.x);
Object.defineProperty(r, "x", { value: 0.5 });
console.log(r.x, q.x + r.x);
function total(ps: P[]): number { let t = 0; for (const p of ps) t += p.x; return t; }
console.log(total([q, r, new P()]));

class Box {
  s: string; o: Box | null; xs: number[]; b: boolean; a: any;
  constructor(s: string) { this.s = s; this.o = null; this.xs = [1]; this.b = false; this.a = 1; }
}
const inner = new Box("inner");
const k = new Box("k");
Object.defineProperty(k, "o", { get() { return inner; }, enumerable: true });
Object.defineProperty(k, "xs", { get() { return [7, 8, 9]; }, enumerable: true });
console.log(JSON.stringify(k));
console.log(Object.values(k).length, JSON.stringify(Object.entries(k)));
console.log(JSON.stringify({ k: k }), JSON.stringify([k]));

class Pair { a: Box | null; b: Box | null; constructor() { this.a = null; this.b = null; } }
const pr = new Pair();
Object.defineProperty(pr, "a", { get() { return inner; }, enumerable: true });
console.log(JSON.stringify(pr));
const vs = Object.values(pr);
console.log(vs.length, vs[0] === inner);
console.log(JSON.stringify(Object.entries(pr)));
const copy = { ...pr };
console.log(copy.a === inner, copy.b);
const tgt = new Pair();
Object.assign(tgt, pr);
console.log(tgt.a === inner);

class Calc {
  g: (...xs: number[]) => number;
  constructor() { this.g = (...xs: number[]) => xs.length; }
}
const c = new Calc();
console.log(c.g(1, 2, 3));
Object.defineProperty(c, "g", { get() { return 7; } });
try { console.log(c.g(1, 2)); } catch (e) { console.log((e as any) instanceof TypeError); }
