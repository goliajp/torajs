// A getter that throws is caught by the caller's try; Reflect and
// defineProperties install accessors too; an object literal's typed
// fields and the optional-chain read take the same path.
class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }
function norm(p: P): number { return p.x * p.x + p.y * p.y; }
function tryRead(p: P): string {
  try { return "ok " + norm(p); } catch (e) { return "caught " + (e as Error).message; }
}
const bad = new P(1, 2);
Object.defineProperty(bad, "x", { get() { throw new Error("nope"); } });
console.log(tryRead(bad), tryRead(new P(3, 4)));
const r = new P(1, 1);
Reflect.defineProperty(r, "y", { get() { return 10; } });
Object.defineProperties(r, { x: { get() { return 20; } } });
console.log(norm(r));
const o = { v: 1, w: 2 };
Object.defineProperty(o, "v", { get() { return 99; }, set(z: number) { console.log("set", z); } });
function bump(t: { v: number; w: number }): number { t.v = t.w + 5; return t.v + t.w; }
console.log(bump(o), bump({ v: 0, w: 1 }));
const q = new P(5, 6);
Object.defineProperty(q, "y", { get() { return 66; } });
const mq: P | undefined = q;
console.log(mq?.y, mq?.x);
