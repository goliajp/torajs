// an `any` value handed to a class-typed parameter through a function
// value: a binding holding a declared function, a callback parameter
// and a local closure. Each value binds as passed and `p.x` reads
// whatever it holds: an instance, an object literal, a JSON.parse
// result, an instance of an unrelated class of the same layout, a
// number, `undefined` (a TypeError at the read)

class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
class Q {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
function show(p: P): string {
  return "x=" + p.x;
}
function bump(p: P): void {
  p.x = p.x + 1;
}
function hof(cb: (p: P) => string, a: any): string {
  return cb(a);
}
function viaLocal(a: any): string {
  const g = (p: P): string => "g=" + p.x;
  return g(a);
}
const f: (p: P) => string = show;

function attempt(label: string, run: () => string): void {
  try {
    console.log(label, run());
  } catch (e) {
    console.log(label, e instanceof TypeError ? "TypeError" : "other");
  }
}

const vals: any[] = [new P(1), { x: 2 }, JSON.parse('{"x":3}'), new Q(4), 5, undefined];
for (const v of vals) {
  attempt("value", () => f(v));
  attempt("callback", () => hof(show, v));
  attempt("arrow-cb", () => hof((p: P) => "a=" + p.x, v));
  attempt("local", () => viaLocal(v));
}

let seen: any = null;
function grab(p: P): string {
  seen = p;
  return "grab";
}
const fg: (p: P) => string = grab;
for (const v of [new P(10), { x: 20 }, JSON.parse('{"x":30}')]) {
  const o: any = v;
  console.log(fg(o));
  bump(o);
  console.log(seen === o, o.x);
  seen = null;
  console.log(hof(grab, o));
  bump(o);
  console.log(seen === o, o.x);
}

// a program-level binding reassigned between declared functions, a
// local one reassigned between closures, a closure a call returns
let m: (p: P) => string = show;
if (vals.length > 99) m = grab;
console.log(m({ x: 60 }), m(JSON.parse('{"x":61}')));
function pick(a: any, k: number): string {
  let g = (p: P): string => "g=" + p.x;
  if (k > 1) g = (p: P): string => "h=" + (p.x + k);
  return g(a);
}
console.log(pick({ x: 3 }, 0), pick({ x: 3 }, 5), pick(7, 5));
function scale(k: number): (p: P) => number {
  return (p: P) => p.x * k;
}
const twice = scale(2);
const w: any = { x: 4 };
console.log(twice(w), twice(JSON.parse('{"x":"s"}')), twice(new Q(5)));
function localGrab(o: any): void {
  let held: any = null;
  const keep = (p: P): string => {
    held = p;
    return "keep";
  };
  console.log(keep(o));
  bump(o);
  console.log(held === o, o.x);
}
localGrab(new P(40));
localGrab({ x: 50 });
