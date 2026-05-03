// Phase M-OO.6 — abstract classes + abstract methods.
//
// `abstract class C { ... }` declares a class that cannot be
// instantiated directly via `new`. Member `abstract method(): T;`
// (no body, semi-terminated) declares a contract that every
// concrete subclass must implement.
//
// Enforcement (desugar_classes):
//   - Abstract method body is empty; no `__cm_<C>__<m>` is emitted.
//     The concrete subclass override provides the actual body.
//   - Concrete subclasses are validated to override every abstract
//     method along the inheritance chain. Missing → compile error.
//   - `new AbstractClass()` is rejected at desugar time (before SSA).
//
// This case shows the canonical Shape / Circle / Square pattern.

abstract class Shape {
  // Concrete field shared by all shapes.
  label: string;
  constructor(l: string) {
    this.label = l;
  }
  // Abstract: subclasses must implement.
  abstract area(): number;
  // Concrete instance method that calls the abstract one — virtual
  // dispatch via the existing __dispatch_area tag-switch (M-OO.3).
  describe(): string {
    return this.label;
  }
}

class Circle extends Shape {
  radius: number;
  constructor(r: number) {
    super("circle");
    this.radius = r;
  }
  area(): number {
    return this.radius * this.radius * 3;
  }
}

class Square extends Shape {
  side: number;
  constructor(s: number) {
    super("square");
    this.side = s;
  }
  area(): number {
    return this.side * this.side;
  }
}

function check(): number {
  let c: Circle = new Circle(4);
  if (c.area() !== 48) { throw "#1: circle area"; }
  if (c.describe() !== "circle") { throw "#2: circle describe"; }

  let s: Square = new Square(5);
  if (s.area() !== 25) { throw "#3: square area"; }
  if (s.describe() !== "square") { throw "#4: square describe"; }

  // Upcast to Shape variable — virtual dispatch picks subclass area().
  // (Mixed-subtype array `Shape[] = [Circle, Square]` would need element-
  // type-driven inference that torajs doesn't run on array literals;
  // separate variables exercise the same dispatch.)
  let s1: Shape = new Circle(2);
  let s2: Shape = new Square(3);
  if (s1.area() !== 12) { throw "#5: s1 (Circle) dispatch"; }
  if (s2.area() !== 9) { throw "#6: s2 (Square) dispatch"; }
  if (s1.describe() !== "circle") { throw "#7: s1 describe"; }
  if (s2.describe() !== "square") { throw "#8: s2 describe"; }

  return 0;
}
console.log(check());
