// chunk 811 — ctor-less derived class synthesizes the ES §15.7.10
// implicit default ctor (params = nearest explicit ancestor ctor's,
// body = super-forward), so `new B(args)` works and a zero-param
// parent ctor's body actually runs.
class A { v: number; constructor(v: number) { this.v = v } }
class B extends A {}
console.log(new B(7).v);

// multi-param + string
class P { x: number; y: string; constructor(x: number, y: string) { this.x = x; this.y = y } }
class Q extends P {}
const q = new Q(3, "hi");
console.log(q.x, q.y);

// two ctor-less hops resolve to the same ancestor ctor
class C extends B {}
console.log(new C(9).v);

// inherited + own methods still dispatch
class M { v: number; constructor(v: number) { this.v = v } greet(): number { return this.v * 2 } }
class N extends M { double(): number { return this.greet() + 1 } }
console.log(new N(5).double());

// spread into the synthesized ctor (rides the factory arity)
const arr: number[] = [7];
console.log(new B(...arr).v);

// zero-param parent ctor body runs (was silently elided pre-fix)
class Z { v: number = 0; constructor() { this.v = 42 } }
class Y extends Z {}
console.log(new Y().v);

// defaulted parent ctor param
class D { v: number; constructor(v: number = 5) { this.v = v } }
class E extends D {}
console.log(new E().v, new E(9).v);

// explicit-ctor derived unchanged
class F extends A { w: number; constructor(v: number, w: number) { super(v); this.w = w } }
const f = new F(1, 2);
console.log(f.v, f.w);

// fully ctor-less chain unchanged
class G {}
class H extends G {}
console.log(typeof new H());
