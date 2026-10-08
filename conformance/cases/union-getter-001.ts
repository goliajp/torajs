// A getter (and a setter-backed property) counts as a member of each
// class in a union receiver (TS §4.13), also when it is inherited.

class Shape {
  constructor(public name: string) {}
  get label(): string {
    return "<" + this.name + ">";
  }
}

class Square extends Shape {
  constructor(public side: number) {
    super("square");
  }
  get area(): number {
    return this.side * this.side;
  }
}

class Circle extends Shape {
  constructor(public r: number) {
    super("circle");
  }
  get area(): number {
    return 3 * this.r * this.r;
  }
}

class Tri {
  private _area = 0;
  label = "tri";
  get area(): number {
    return this._area;
  }
  set area(v: number) {
    this._area = v;
  }
}

function describe(s: Square | Circle): string {
  return s.label + " " + s.area;
}

function areaOf(s: Square | Circle | Tri): number {
  return s.area;
}

const t = new Tri();
t.area = 12;
console.log(describe(new Square(2)), describe(new Circle(1)));
console.log(areaOf(new Square(3)), areaOf(new Circle(2)), areaOf(t));
