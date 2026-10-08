// An array typed by an interface holding instances of two unrelated
// classes: each element answers through its own class.
interface Shape {
  area(): number;
}
class Sq {
  s: number;
  constructor(s: number) {
    this.s = s;
  }
  area() {
    return this.s * this.s;
  }
}
class Ci {
  r: number;
  constructor(r: number) {
    this.r = r;
  }
  area() {
    return 3.14 * this.r * this.r;
  }
}
const shapes: Shape[] = [new Sq(2), new Ci(1)];
for (const x of shapes) console.log(x.area());
shapes.push(new Ci(2), new Sq(5));
let sum = 0;
for (let i = 0; i < shapes.length; i++) sum += shapes[i].area();
console.log(shapes.length, sum);
console.log(shapes.map((s) => s.area()).join(" "));
