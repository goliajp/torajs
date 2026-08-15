// L3b ② — a GENERIC base class in the override chain: `__dispatch_`
// interception resolves the slot signature through the checker's mono
// retarget, mono factories carry their own vtable rows, and the width
// union spans both the suffixed and bare `__cm_` spellings.
class Shape<T> {
  tag: T;
  constructor(tag: T) {
    this.tag = tag;
  }
  area(): number {
    return 0;
  }
  kind(): string {
    return "shape";
  }
}
class Circle extends Shape<number> {
  area(): number {
    return 3.14 * this.tag * this.tag;
  }
  kind(): string {
    return "circle";
  }
}
class Square extends Shape<number> {
  area(): number {
    return this.tag * this.tag;
  }
  kind(): string {
    return "square";
  }
}
const c = new Circle(2);
const s = new Square(3);
console.log(c.kind(), c.area());
console.log(s.kind(), s.area());
const shapes: Shape<number>[] = [new Shape(1), c, s];
for (const sh of shapes) {
  console.log(sh.kind(), sh.area());
}

class Animal<T> {
  name: T;
  constructor(name: T) {
    this.name = name;
  }
  speak(): string {
    return "generic " + this.name;
  }
}
class Dog extends Animal<string> {
  speak(): string {
    return super.speak() + " woof";
  }
}
function run(): void {
  const d = new Dog("rex");
  console.log(d.speak());
  const a: Animal<string> = d;
  console.log(a.speak());
  const g = new Animal<string>("misc");
  console.log(g.speak());
}
run();
