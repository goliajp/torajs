// An `any` value written into a slot the program annotated with a class
// or an object type, when the value is not something the slot can hold
// as it is, throws a TypeError at the write. bun runs these writes
// (an annotation does nothing at run time), so this case is checked
// against its .expected: the throw is a deliberate departure: an
// annotated slot holds what its annotation says.
// A class slot takes an instance of the class or of a subclass only, an
// object-type slot an object with its fields, or a fresh object nothing
// else holds whose properties are exactly those fields.

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
type T = { x: number };
const shared: any = { x: 1 };
// one value per call: an element read of an `any[]` would retype the
// arrays it is pushed into as `any[]` before any cast
function pick(k: number): any {
  if (k === 0) return new Q(1);
  if (k === 1) return shared;
  if (k === 2) return JSON.parse('{"x":2}');
  if (k === 3) return 3;
  if (k === 4) return "four";
  if (k === 5) return [5];
  if (k === 6) return undefined;
  return null;
}
function attempt(tag: string, f: () => void): void {
  try {
    f();
    console.log(tag, "ok");
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message);
  }
}

function ret(v: any): P {
  return v;
}
function retT(v: any): T {
  return v;
}
class Holder {
  p: P;
  constructor() {
    this.p = new P(0);
  }
}
let g: P = new P(0);
for (let k = 0; k < 8; k++) {
  const o = pick(k);
  attempt("return", () => ret(o));
  attempt("field", () => {
    new Holder().p = o;
  });
  attempt("local", () => {
    let q: P = new P(0);
    q = o;
  });
  attempt("global", () => {
    g = o;
  });
  attempt("push", () => {
    const xs: P[] = [];
    xs.push(o);
  });
  attempt("decl", () => {
    const d: P = o;
  });
  attempt("object type", () => retT(o));
}
function fresh(src: string): T {
  return JSON.parse(src);
}
attempt("extra property", () => fresh('{"x":1,"y":2}'));
attempt("missing property", () => fresh("{}"));
attempt("property type", () => fresh('{"x":"1"}'));
attempt("fresh", () => console.log(fresh('{"x":9}').x));
// a class slot takes an instance only: a fresh object is never rebuilt
// as one, whatever its properties
function litP(): P {
  return { x: 1 } as any;
}
function parseP(src: string): P {
  return JSON.parse(src);
}
attempt("fresh literal into class", () => litP());
attempt("fresh parse into class", () => parseP('{"x":2}'));
