// a callback parameter only ever handed declared functions, called
// with an `any` argument for its class-typed parameter: a parameter
// annotated `any`, a local annotated `any`, an `as any`. Each value
// binds as passed and `p.x` reads whatever it holds

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
let seen: any = null;
function grab(p: P): string {
  seen = p;
  return "grab";
}
function hof(cb: (p: P) => string, a: any): string {
  return cb(a);
}
function apply(cb: (p: P) => void, a: any): void {
  cb(a as any);
}
function parsed(cb: (p: P) => string, s: string): string {
  const v: any = JSON.parse(s);
  return cb(v);
}

const vals: any[] = [new P(1), { x: 2 }, JSON.parse('{"x":3}'), new Q(4), 5, undefined];
for (const v of vals) {
  try {
    console.log(hof(show, v));
  } catch (e) {
    console.log(e instanceof TypeError ? "TypeError" : "other");
  }
}
for (const v of [new P(10), { x: 20 }, JSON.parse('{"x":30}')]) {
  const o: any = v;
  console.log(hof(grab, o));
  apply(bump, o);
  console.log(seen === o, o.x);
  seen = null;
}
console.log(parsed(show, '{"x":9}'), parsed(show, '{"x":"s"}'));
