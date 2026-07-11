// chunk 812 — `C.length` per ES §15.7.13: the ctor's expected
// argument count (formal params before the first default / rest);
// a synthesized derived default ctor is rest-shaped per spec, so 0.
class A { v: number; constructor(v: number) { this.v = v } }
class B extends A {}
class C {}
class D { a: number; b: number; constructor(a: number, b: number = 1) { this.a = a; this.b = b } }
class F extends A { w: number; constructor(v: number, w: number) { super(v); this.w = w } }
console.log(A.length, B.length, C.length, D.length, F.length);

// name / prototype siblings unchanged
console.log(A.name, B.name);
console.log(A.prototype === A.prototype);

// two-hop ctor-less chain also 0
class G extends B {}
console.log(G.length);
