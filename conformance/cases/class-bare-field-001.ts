// p1 — bare instance field: undefined until written
class C1 { x; m() { return 42; } }
var c1 = new C1();
console.log(c1.x);
c1.x = 7;
console.log(c1.x);
console.log(c1.m());

// p2 — multiple bare fields + mixed with initialized
class C2 { a; b = 5; c; }
var c2 = new C2();
console.log(c2.a, c2.b, c2.c);

// p3 — bare private field readable inside
class C3 {
  #p;
  get_p() { return this.#p; }
  set_p(v: any) { this.#p = v; }
}
var c3 = new C3();
console.log(c3.get_p());
c3.set_p(9);
console.log(c3.get_p());

// p4 — bare static field
class C4 { static s; }
console.log(C4.s);

// p5 — bare field ending at brace (ASI-free form)
class C5 { y }
console.log(new C5().y);
