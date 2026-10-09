// A conditional with an `undefined` arm (or two unrelated classes) in
// a position declared with an interface or class type: each arm is
// checked against the declared type and stored as the object itself.
interface Shape {
  area(): number;
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
class Ci implements Shape {
  r: number;
  constructor(r: number) {
    this.r = r;
  }
  area() {
    return 3 * this.r * this.r;
  }
}
const n = process.argv.length;
function pick(i: number): Shape | undefined {
  return i % 3 === 0 ? undefined : i % 3 === 1 ? new Sq(i) : new Ci(i);
}
let total = 0;
let none = 0;
for (let i = 0; i < 9; i++) {
  const s = pick(i);
  if (s === undefined) none++;
  else total += s.area();
}
console.log(total, none);
const a: Shape | undefined = n > 100 ? new Sq(3) : undefined;
console.log(a === undefined, typeof a, a);
const b: Shape | undefined = n > 100 ? undefined : new Sq(4);
console.log(b === undefined, b === null, b?.area());
const c: Shape = n > 100 ? new Sq(5) : new Ci(2);
console.log(c.area());
const d: Sq | undefined = n > 100 ? new Sq(6) : undefined;
console.log(d === undefined, d?.area());
const e: Shape | null = n > 100 ? null : new Ci(1);
console.log(e === null, e?.area());
let last: Shape | undefined = undefined;
for (let i = 0; i < 4; i++) {
  const v: Shape | undefined = i % 2 === 0 ? undefined : new Sq(i);
  if (v !== undefined) last = v;
}
console.log(last === undefined ? -1 : last.area());
// a null arm, taken and not taken, on either side
function orNull(i: number): Shape | null {
  return i % 2 === 0 ? null : new Sq(i);
}
function nullOr(i: number): Shape | null {
  return i % 2 === 0 ? new Ci(i) : null;
}
let shown = "";
for (let i = 0; i < 4; i++) {
  const p = orNull(i);
  const q = nullOr(i);
  shown += (p === null ? "n" : p.area()) + "/" + (q === null ? "n" : q.area()) + " ";
}
console.log(shown);
const f: Shape | null = n < 100 ? null : new Sq(7);
const g: Shape | null = n > 100 ? new Ci(7) : null;
console.log(f === null, g === null, f?.area(), g?.area() ?? "none");
