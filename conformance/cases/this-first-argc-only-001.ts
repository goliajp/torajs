// A this-first body (a promoted `this`-reading declaration, a
// constructor, a static method's receiver twin) that reads only
// `arguments.length` is on the argc tier alone: its direct-call sites
// insert the count right after the head and no argv buffer, so a
// relay's forwarding call passes its true runtime count through
// uncapped. Before the split every admitted body took the argv
// channel, the relay capped the count at the slots it packed (its
// declared arity), and a TYPED HOF site reaching such a body through
// the receiver-first relay answered the declared arity: `[7].map(t,
// o)` said 1 where bun says 3 (rotation 650; the any-lane route was
// already right since rotation 649).
const o = { k: 1 };
function t(this: any, a: number): any { return [arguments.length, a, (this as any).k]; }
console.log(JSON.stringify([7].map(t, o)));
console.log(JSON.stringify([7, 8].filter(t, o)));
[8].forEach(t, o);
console.log(JSON.stringify([1.5].map(t, o)));
console.log(JSON.stringify(t.call(o, 5, 6, 7)), JSON.stringify(t.call(o, 5)));
function cnt(this: any): number { return arguments.length; }
console.log([1, 2].every(cnt, o), [5].some(cnt, o), cnt.call(o), cnt.call(o, 1, 2, 3, 4));
class P { n: number; constructor() { this.n = arguments.length; } }
class C extends P { constructor(a: number, b: number) { super(a, b); } }
class D extends P { constructor() { super(); } }
console.log(new C(1, 2).n, new D().n);
class S { static g(this: any, a: number) { return [this === S, arguments.length, a]; } }
console.log(JSON.stringify(S.g.call({}, 1, 2)), JSON.stringify(S.g.call({}, 7)), JSON.stringify(S.g(1, 2, 3)));
// A length WRITE stays on the argc tier (the hidden slot seeds a
// mutable local). A body reading VALUES stays on the argv tier, whose
// HOF-callback routes through the relay are a recorded residue; only
// its direct route is read here.
function w(this: any, a: number): any { arguments.length = 9; return [arguments.length, a, (this as any).k]; }
console.log(JSON.stringify(w.call(o, 1, 2)), JSON.stringify(w.call(o, 1)));
function v(this: any, a: number): any { return [arguments.length, arguments[1], (this as any).k]; }
console.log(JSON.stringify(v.call(o, 1, 2, 3)), JSON.stringify(v.call(o, 1)));
const xs: any = [7];
console.log(JSON.stringify(xs.map(t, o)), JSON.stringify(xs.filter(t, o)));
