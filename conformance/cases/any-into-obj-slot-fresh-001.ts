// A fresh object (a JSON.parse result or an object literal nothing else
// holds) written through `any` into a slot annotated with an object type
// is the object with exactly those fields. A `number` field takes the
// fraction the text holds at every kind of write, however the program's
// own writes to the type look.

type T = { x: number; s: string };
function show(tag: string, f: () => T): void {
  const t = f();
  console.log(tag, t.x, t.s, JSON.stringify(t), Object.keys(t).join(","));
}
const seed: T = { x: 1, s: "a" };
function ret(src: string): T {
  return JSON.parse(src);
}
function lit(): T {
  return { x: 1.25, s: "o" } as any;
}
show("return", () => ret('{"x":2.5,"s":"r"}'));
show("literal", lit);
show("let", () => {
  const t: T = JSON.parse('{"x":3.5,"s":"l"}');
  return t;
});
show("let from call", () => {
  const t: T = ret('{"x":4.5,"s":"c"}');
  return t;
});
let g: T = { x: 0, s: "g" };
show("global", () => {
  g = JSON.parse('{"x":5.5,"s":"g"}');
  return g;
});
show("local", () => {
  let t: T = { x: 0, s: "z" };
  t = JSON.parse('{"x":6.5,"s":"z"}');
  return t;
});
class H {
  t: T;
  constructor() {
    this.t = { x: 0, s: "h" };
  }
}
show("field", () => {
  const h = new H();
  h.t = JSON.parse('{"x":7.5,"s":"f"}');
  return h.t;
});
show("push", () => {
  const xs: T[] = [{ x: 0, s: "p" }];
  xs.push(JSON.parse('{"x":8.5,"s":"p"}'));
  return xs[1];
});
show("integer", () => ret('{"x":9,"s":"i"}'));
console.log(seed.x + 1, g.x);

const o: { x: number } = JSON.parse('{"x":2.5}');
console.log(o.x);
