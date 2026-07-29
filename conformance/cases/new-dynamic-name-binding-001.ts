// A bare name after `new` is only a factory when it names a class.
// When it names a binding that holds one, the constructor is a value
// and the answer is the same.
class Shape {
  sides: number;
  constructor(sides: number) {
    this.sides = sides;
  }
}

const Alias: any = Shape;
const tri = new Alias(3);
console.log(tri.sides);

function pick(flag: boolean): any {
  return flag ? Shape : Shape;
}
const chosen: any = pick(true);
console.log(new chosen(4).sides);

// ES §13.3.5.1 step 7 — not a constructor, so TypeError.
const notCtor: any = { plain: 1 };
try {
  const bad = new notCtor();
  console.log("built " + String(bad));
} catch (e: any) {
  console.log(e instanceof TypeError);
}
