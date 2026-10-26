// An element of a function array whose element type admits `undefined`
// (held boxed) bound to an annotated `F | undefined` binding, assigned to
// one, or handed to an `F | undefined` parameter: `undefined` stays
// `undefined`, a function calls, and one stored as `any` answers what it
// returns.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const hs: (F | undefined)[] = [(x: number) => x + 1, undefined, (x: number) => x * 3];

const a: F | undefined = hs[0];
const u: F | undefined = hs[1];
let c: F | undefined = hs[2];
console.log(a ? a(1) : "none", u === undefined, typeof u, u ? u(1) : "none", c!(2));
c = hs[1];
console.log(c === undefined, typeof c);
c = hs[0];
if (c) console.log(c(5));
const d: F | undefined = hs.at(2);
const e: F | undefined = hs.find((f) => f !== undefined);
console.log(d!(4), e!(4), hs.at(9) === undefined);

function local(): number {
  const z: F | undefined = hs[2];
  let y: F | undefined = hs[1];
  y = hs[0];
  return (z ? z(7) : -1) + (y ? y(1) : -1);
}
console.log(local());

function runU(f: F | undefined) {
  return f ? f(4) : "none";
}
console.log(runU(hs[0]), runU(hs[1]), runU(hs.at(2)));

hs[1] = tag;
const b: F | undefined = hs[1];
if (b) console.log(b(2));
let m: F | undefined = hs[0];
m = hs[1];
console.log(m ? m(6) : "none", runU(hs[1]));
const p: F | undefined = hs.pop();
console.log(p!(9), hs.length);
