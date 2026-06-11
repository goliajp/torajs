// W4 D3 (ann-width RFC §5.4) — struct / class field width follows the
// alias class (repro S3 field face): fractional values round-trip as
// f64 through annotated object fields, class fields join over all
// instances through the nominal class, and int-only fields keep the
// narrow i64 representation.

// s3f — inline object field, fract assign after int init
let o: { x: number } = { x: 1 };
o.x = 0.5;
console.log(o.x);

// s3g — object literal fract init
let p: { x: number } = { x: 0.5 };
console.log(p.x);

// int-only field holds the narrow face
let q: { n: number } = { n: 3 };
q.n = 7;
console.log(q.n);

// s3h — class field via instance assign
class P {
  x: number;
  constructor() {
    this.x = 1;
  }
}
let c1 = new P();
c1.x = 0.5;
console.log(c1.x);

// class field written from a method body joins the same class
class Q {
  v: number;
  constructor() {
    this.v = 2;
  }
  half(): void {
    this.v = this.v / 4;
  }
}
let c2 = new Q();
c2.half();
console.log(c2.v);

// field read flows into a number slot
let acc: number = o.x + p.x;
console.log(acc);

// array inside a struct field
let holder: { xs: number[] } = { xs: [1, 2] };
holder.xs[0] = 0.5;
console.log(holder.xs[0]);
holder.xs.push(2.5);
console.log(holder.xs[2]);

// struct passed through a fn boundary
function bump(t: { x: number }): number {
  t.x = t.x + 0.25;
  return t.x;
}
let s: { x: number } = { x: 1 };
console.log(bump(s));
console.log(s.x);
