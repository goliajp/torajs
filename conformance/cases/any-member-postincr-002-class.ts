class D {
  c = 0;
  bump() { this.c++; return this.c; }
  drop2() { this.c--; this.c--; return this.c; }
}
const d = new D();
console.log(d.bump());
console.log(d.bump());
console.log(d.drop2());
const e: any = new D();
console.log(e.c++);
console.log(e.c);
