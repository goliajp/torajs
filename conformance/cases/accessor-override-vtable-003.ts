// three levels, compound assignment and super.n through a base receiver
class A {
  _n = 1;
  get n(): number { return this._n; }
  set n(x: number) { this._n = x; }
}
class M extends A {}
class L extends M {
  get n(): number { return super.n * 10; }
  set n(x: number) { super.n = x + 100; }
}
function read(a: A): number { return a.n; }
function bump(a: A): number { a.n++; return a._n; }
function add(a: A): number { a.n += 1; return a._n; }
console.log("read", read(new A()), read(new M()), read(new L()));
console.log("inc", bump(new A()), bump(new M()), bump(new L()));
console.log("add", add(new A()), add(new M()), add(new L()));
const ms: M[] = [new M(), new L()];
console.log("mid", ms.map((m) => m.n).join(","));
let x: A = new L();
x.n = 2;
console.log("assign", x._n, x.n);
