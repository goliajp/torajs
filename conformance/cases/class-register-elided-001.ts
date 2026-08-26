// RFC 20260824-s2-5 刀 4 A8 — a class program whose class and
// prototype cells never leave their prologue and whose instances
// never enter the any world: the link judges the register call away
// (no registry reader is live, the cells are private) and drops the
// method rows' any-lane twins. Everything below must still run for
// real: typed method calls, field reads, a subclass chain, and the
// struct inspect that lists the prototype methods off the rows the
// judgment keeps.
class Point {
  x = 1;
  y = 2;
  scaled(k: number): number {
    return (this.x + this.y) * k;
  }
  sum(): number {
    return this.x + this.y;
  }
}
class Point3 extends Point {
  z = 3;
  all(): number {
    return this.sum() + this.z;
  }
}
const p = new Point();
console.log(p.sum(), p.scaled(10));
const q = new Point3();
console.log(q.all(), q.scaled(2));
console.log(p);
console.log(q);
let acc = 0;
for (let i = 0; i < 1000; i++) {
  const t = new Point3();
  t.z = i;
  acc += t.all();
}
console.log(acc);
