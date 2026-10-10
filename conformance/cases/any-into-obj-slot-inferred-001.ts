// A slot whose type the compiler inferred takes an `any` value of any
// kind as it is, at every write: a reassigned local or module binding,
// an element appended to an array, a field of an object literal. Reads
// see the value itself.

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
function vals(): any[] {
  return [new P(1), new Q(2), { x: 3 }, JSON.parse('{"x":4}'), 5, "six", [7], undefined, null];
}
function show(tag: string, got: any, o: any): void {
  const kind = got === null ? "null" : Array.isArray(got) ? "array" : typeof got;
  console.log(tag, got === o, kind, got instanceof P);
}

function local(o: any): void {
  let q = new P(0);
  q = o;
  show("local", q, o);
}
for (const o of vals()) local(o);

let g = new P(0);
function global(o: any): void {
  g = o;
  show("global", g, o);
}
for (const o of vals()) global(o);

for (const o of vals()) {
  const xs = [new P(0)];
  xs.push(o);
  show("push", xs[1], o);
}

for (const o of vals()) {
  const lit = { p: new P(0), n: 1 };
  lit.p = o;
  show("literal field", lit.p, o);
}
