// 563-07 — TS's definite assignment assertion, `v!: T`.
//
// The parser rejected it outright ("expected `(` (method) or `:`
// (field) after `v`"). It is a claim addressed to the type checker
// — "this IS assigned before any read" — with no runtime face and,
// unlike `?`, no effect on the declared type: `v!: number` is
// `number`, not `number | undefined`. So the two markers share
// exactly one effect (each shifts every downstream cursor by one)
// and differ in the other, which is what `FieldMarker` now says
// where a single `optional: bool` used to.
class G {
  v!: number;
  private p!: string;
  readonly r!: boolean;
  set() { this.v = 1; this.p = "x"; (this as any).r = true }
  read() { return [this.v, this.p, this.r] }
}
const g = new G();
g.set();
console.log(g.read());
console.log(Object.getOwnPropertyNames(g));

// the declared type is NOT widened — an optional twin for contrast
class H { a!: number; b?: number }
const h: any = new H();
h.a = 2;
console.log(h.a, h.b);

// generic classes carry it through monomorphisation
class L<T> {
  v!: T;
  set(x: T) { this.v = x }
}
const li = new L<number>();
li.set(3);
const ls = new L<string>();
ls.set("s");
console.log(li.v, ls.v);

// the same marker on a variable declaration
let x!: number;
x = 4;
console.log(x);
var y!: string;
y = "z";
console.log(y);
