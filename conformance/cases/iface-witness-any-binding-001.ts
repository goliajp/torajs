// A binding annotated with an interface (or another object type) and
// initialized from an `any` value holds whatever that value is: an
// object literal with no witness row, a class instance, `undefined`.
interface Shape {
  area(): number;
  name?: string;
}
class Sq implements Shape {
  s: number;
  constructor(s: number) {
    this.s = s;
  }
  area() {
    return this.s * this.s;
  }
}
const q: Shape = new Sq(2);
console.log(q.area());
const raw: any = { area: () => 5, name: "lit" };
const s: Shape = raw;
console.log(s.area(), s.name);
function readTop(): number {
  return s.area() + 1;
}
console.log(readTop());
function inner(v: any): string {
  const t: Shape | undefined = v;
  return t === undefined ? "none" : t.area() + ":" + t.name;
}
console.log(inner(raw), inner(undefined), inner(new Sq(3)));
const boxed: any = new Sq(4);
const u: Shape = boxed;
console.log(u.area());

interface Pt {
  x: number;
  y: number;
}
const parsed: any = JSON.parse('{"x":1,"y":2}');
const p: Pt = parsed;
console.log(p.x + p.y, p.x, p.y);
const direct: Pt = JSON.parse('{"x":3,"y":4}');
console.log(direct.x * direct.y);
const bag: any = { inner: { x: 7, y: 8 } };
const fromProp: Pt = bag.inner;
console.log(fromProp.y - fromProp.x);

// The binding is still a `Pt`: its members read as numbers, while the
// whole value handed on is the box it holds, wherever it goes.
function early(): number {
  return yOf(p) + p.x;
}
function yOf(t: Pt): number {
  return t.y;
}
console.log(early(), yOf(fromProp));
const again: Pt = p;
const listed: Pt[] = [p, { x: 5, y: 6 }];
console.log(again.x * again.y, listed[0].y + listed[1].x, { inner: p }.inner.x);
const late = (): number => yOf(p) * 10 + p.y;
console.log(late());
function local(v: any): number {
  const q: Pt = v;
  const r: Pt | undefined = v;
  const g = (): number => yOf(q) + q.x;
  return r === undefined ? -1 : g() + r.y;
}
console.log(local(parsed), local(undefined));
console.log(JSON.stringify(p), p === parsed, typeof p);
p.x = 10;
console.log(parsed.x, again.x);
