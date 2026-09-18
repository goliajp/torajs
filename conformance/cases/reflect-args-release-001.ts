// Reflect.apply / Reflect.construct borrow their operands; an owned temp
// among them (an array literal as the argument list, a fresh call product
// as the list or the receiver, a surplus fourth argument) had no release
// site and stranded one block per call. A shared list keeps its stake and
// reads back intact; a repeated target (the two-argument construct form)
// is released once; the kernel's own throws still propagate.
class F { v: number; w: string; constructor(v: number, w: string) { this.v = v; this.w = w; } }
function f(this: any, a: number, b: string): string { return (this ? this.tag : "no") + ":" + a + b; }
function mk(i: number): any[] { return [i, "m" + i]; }
function run(i: number): void {
  const o: any = Reflect.construct(F, [i, "x" + i]); console.log(o.v, o.w, o instanceof F);
  const p: any = Reflect.construct(F, mk(i)); console.log(p.v, p.w);
  const shared: any[] = [i + 1, "s" + i]; const q: any = Reflect.construct(F, shared); console.log(q.v, q.w, shared.length, shared[1]);
  console.log(Reflect.apply(f, { tag: "t" + i }, [i, "a"]), Reflect.apply(f, null, mk(i)), Reflect.apply(f, { tag: "u" }, shared, { extra: "e" + i }));
  const t = "zz" + i; console.log(shared[1], t);
  try { Reflect.construct(F, null as any); } catch (e: any) { console.log("caught", e instanceof TypeError); }
  try { Reflect.apply(f, null, 5 as any); } catch (e: any) { console.log("caught", e instanceof TypeError); }
}
for (let i = 1; i <= 3; i++) run(i);
