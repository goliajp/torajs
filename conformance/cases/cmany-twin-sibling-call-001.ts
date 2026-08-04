class C {
  x = 1;
  m() { return this.x; }
  n() { return this.m() + 1; }
}
class Sub extends C {
  m() { return this.x + 10; }
}
const c = new C();
console.log(c.n());
const s = new Sub();
console.log(s.n());
