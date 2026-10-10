// An `any` value written into a slot the program annotated with a class
// or an object type is the value itself: a return, a field, a local or
// module binding, an element appended by push, a declaration. An
// instance of the class or of a subclass keeps its identity (a write
// through one name is seen through the other), and a `T | undefined`
// slot takes undefined.

class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
  m(): string {
    return "P.m" + this.x;
  }
}
class S extends P {
  y: number;
  constructor(x: number) {
    super(x);
    this.y = 7;
  }
  m(): string {
    return "S.m" + this.y;
  }
}
function bump(o: any): void {
  o.x = o.x + 100;
}
function report(tag: string, p: P, o: any): void {
  bump(o);
  console.log(tag, p === o, p.x, p.m(), p instanceof S);
}
function vals(): any[] {
  return [new P(1), new S(2)];
}

function ret(v: any): P {
  return v;
}
for (const o of vals()) report("return", ret(o), o);

class Holder {
  p: P;
  constructor() {
    this.p = new P(0);
  }
}
for (const o of vals()) {
  const h = new Holder();
  h.p = o;
  report("field", h.p, o);
}

function local(o: any): void {
  let q: P = new P(0);
  q = o;
  report("local", q, o);
}
for (const o of vals()) local(o);

let g: P = new P(0);
function global(o: any): void {
  g = o;
  report("global", g, o);
}
for (const o of vals()) global(o);

for (const o of vals()) {
  const xs: P[] = [];
  xs.push(o);
  report("push", xs[0], o);
}
function pushInto(xs: P[], o: any): number {
  return xs.push(o);
}
const ps: P[] = [];
for (const o of vals()) {
  pushInto(ps, o);
  report("param push", ps[ps.length - 1], o);
}

for (const o of vals()) {
  const d: P = o;
  report("decl", d, o);
}

function maybe(v: any): P | undefined {
  return v;
}
console.log(maybe(undefined) === undefined, maybe(new S(3))?.m());
let nq: P | null = null;
function setNull(v: any): void {
  nq = v;
}
setNull(null);
console.log(nq === null);
setNull(new P(4));
console.log(nq === null ? "-" : nq.m());
