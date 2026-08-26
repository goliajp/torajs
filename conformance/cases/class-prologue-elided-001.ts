// r505 (A12) — a class program whose cells never leave the prologue:
// the whole prologue (the `__proto_<C>` / `__class_<C>` mints, the
// subclass chain link, the registers) lives in its own synthesized fn,
// main's one call to it rides the registry-reader guard and is NOPed
// with the register sites, and the user-fn dead-strip removes the fn
// — dynobj / anyvalue / str_alloc worlds and all. Everything below
// still runs for real: field inits, a typed override, a static
// method, a compile-time-folded `instanceof`, a loop of mints.
class Shape {
  w = 2;
  h = 3;
  constructor(w: number, h: number) {
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
  static unit(): number {
    return 1;
  }
}
class Square extends Shape {
  constructor(s: number) {
    super(s, s);
  }
  area(): number {
    return this.w * this.w;
  }
  perimeter(): number {
    return 4 * this.w;
  }
}
const s = new Shape(4, 5);
const q = new Square(6);
console.log(s.area(), q.area(), q.perimeter(), Shape.unit());
console.log(q instanceof Shape, s instanceof Square);
let total = 0;
for (let i = 1; i <= 300; i++) {
  total += new Square(i).area();
}
console.log(total);
