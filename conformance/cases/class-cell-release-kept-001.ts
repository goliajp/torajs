// r503 — the kept side of class-cell-release-elided-001: a ctor reads
// `new.target` (a registry reader, `class_get`), so the register call
// and the cells' exit release stay real and the `__new_target` box
// carries a heap cell the mid-end must not touch. The instance also
// crosses into the any world and is compared against the prototype
// through `Object.getPrototypeOf`.
class Shape {
  sides = 0;
  constructor(sides: number) {
    const t: any = new.target;
    console.log(t === Shape, t.name);
    this.sides = sides;
  }
  describe(): string {
    return "shape with " + this.sides + " sides";
  }
}
class Square extends Shape {
  constructor() {
    super(4);
  }
  describe(): string {
    return "square: " + super.describe();
  }
}
const s = new Square();
console.log(s.describe());
const anyS: any = s;
console.log(Object.getPrototypeOf(anyS) === Square.prototype);
console.log(anyS instanceof Shape, anyS.sides);
for (let i = 0; i < 3; i++) {
  const sh = new Shape(i);
  console.log(sh.describe());
}
