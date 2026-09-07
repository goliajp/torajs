// `desugar_classes` rewrites `C.m` in place to a flat `__sm_C__m`
// Ident, so a static method bound to a variable reaches the boxing
// points already spelled synthetic — and it is an ordinary top-level
// fn, safe to relay through its forwarder.
class Shape {
  static area(w: number, h: number): number {
    return w * h;
  }
  static tag(): string {
    return "shape";
  }
}
function takeAny(x: any): number {
  return x(2, 3);
}

const direct = Shape.area;
console.log(direct(4, 5));

const boxed = Shape.area;
console.log(takeAny(boxed));
console.log(boxed(6, 7));
console.log(typeof boxed);

const label = Shape.tag;
const bag: any[] = [];
bag.push(label);
console.log(bag.length, label());
