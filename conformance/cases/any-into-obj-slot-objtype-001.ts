// A slot annotated with an object type (not a class) takes any object
// laid out with its fields as it is, an instance of an unrelated class
// with those fields too; a fresh object literal or JSON.parse result
// nothing else holds becomes the slot's object with the same
// properties, in the same order.

type T = { x: number; s: string };
class C {
  x: number;
  s: string;
  constructor(x: number, s: string) {
    this.x = x;
    this.s = s;
  }
}
function asT(v: any): T {
  return v;
}
const c = new C(1, "c");
const t = asT(c);
t.x = 10;
console.log(t === (c as any), c.x, t.s);

function lit(): T {
  return { x: 2, s: "lit" } as any;
}
function parsed(src: string): T {
  return JSON.parse(src);
}
for (const v of [lit(), parsed('{"x":3,"s":"json"}')]) {
  console.log(v.x, v.s, JSON.stringify(v), Object.keys(v).join(","));
}

let held: T = { x: 0, s: "z" };
function hold(src: string): void {
  held = JSON.parse(src);
}
hold('{"x":4,"s":"held"}');
console.log(held.x + 1, held.s);

class Box {
  t: T;
  constructor() {
    this.t = { x: 0, s: "" };
  }
}
const b = new Box();
b.t = asT(new C(5, "box"));
console.log(b.t.x, b.t.s, b.t instanceof C);
