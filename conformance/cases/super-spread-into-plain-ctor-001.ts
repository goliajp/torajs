// `super(head, ...xs)` into a parent constructor that declares no
// parameters and never reads `arguments`. The runtime width had no
// lane to it — the bare ctor name died loud at the spread lane's
// callee boxing (`box_to_any element type FnSig`) — so such a ctor
// now takes the argv channel whenever a super site spreads, without
// materializing an arguments object its body never reads. The
// spread's own evaluation order and throws stay §13.3.8.1's
// (test262 super/call-spread-err-mult-err-*).
class P { constructor() { } }
class C extends P { constructor() { super(0, ...[1, 2]); } }
console.log(new C() instanceof C, new C() instanceof P);
class Q extends P { constructor() { super(...function* () { throw new Error("boom"); }()); } }
try { new Q(); console.log("no throw"); } catch (e: any) { console.log(e.message); }
const order: string[] = [];
function* g(tag: string) { order.push(tag); yield tag; }
class S extends P { constructor() { super(...g("a"), 5, ...g("b")); order.push("body"); } }
new S(); console.log(order.join(","));
function* late(): Generator<number> { yield 1; throw new Error("late"); }
class R extends P { constructor() { super(1, ...late()); } }
try { new R(); console.log("no throw"); } catch (e: any) { console.log(e.message); }
class T { constructor(...xs: number[]) { console.log("T", xs.length, xs[2]); } }
class U extends T { constructor() { super(...[1, 2, 3]); } }
new U();
class V extends P { constructor() { super(); console.log("V"); } }
new V();
