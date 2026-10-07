// An object-literal getter or setter has an `arguments` object of its
// own wherever its literal stands, never the enclosing function's:
// a returned literal, a call argument, an assignment, a nested literal,
// a conditional, an array element, a computed accessor, and the bodies
// of a generator, a constructor and a generic class method.

function ret(a: number, b: number, c: number) {
  return {
    get g() { return arguments.length + ":" + typeof arguments; },
    set s(v: number) { console.log("ret set", arguments.length, arguments[0]); },
  };
}
const r = ret(1, 2, 3);
console.log("ret", r.g);
r.s = 9;

function show(o: any) {
  console.log("arg", o.g);
  o.s = 5;
}
function callArg(a: number, b: number, c: number) {
  show({
    get g() { return arguments.length + ":" + typeof arguments; },
    set s(v: number) { console.log("arg set", arguments.length, arguments[0]); },
  });
}
callArg(1, 2, 3);

let held: any;
function assign(a: number, b: number, c: number) {
  held = { get g() { return arguments.length + ":" + typeof arguments; } };
  let p: any;
  p = { get h() { return arguments[0] + ":" + arguments.length; } };
  console.log("assign local", p.h);
}
assign(1, 2, 3);
console.log("assign", held.g);

function nested(a: number, b: number, c: number) {
  const o = {
    inner: {
      get g() { return arguments.length + ":" + typeof arguments; },
      set s(v: number) { console.log("nested set", arguments.length, arguments[0]); },
    },
  };
  console.log("nested", o.inner.g);
  o.inner.s = 7;
}
nested(1, 2, 3);

function cond(a: number, b: number, c: number) {
  const o = a > 0
    ? { get g() { return arguments.length + ":" + typeof arguments; } }
    : { get g() { return "neg"; } };
  console.log("cond", o.g);
}
cond(1, 2, 3);

function elem(a: number, b: number, c: number) {
  const xs = [{ get g() { return arguments.length + ":" + typeof arguments; } }];
  console.log("elem", xs[0].g);
}
elem(1, 2, 3);

function computed(a: number, b: number) {
  const k = "g";
  const o: any = { get [k]() { return arguments.length + ":" + typeof arguments; } };
  console.log("computed", o.g);
}
computed(1, 2);

function* gen(a: number, b: number) {
  const o = { get g() { return arguments.length + ":" + typeof arguments; } };
  yield o.g;
}
for (const v of gen(1, 2)) console.log("gen", v);

class C {
  v: string;
  constructor(a: number, b: number) {
    const o = { get g() { return arguments.length + ":" + typeof arguments; } };
    this.v = o.g;
  }
}
console.log("ctor", new C(1, 2).v);

class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  peek(a: number, b: number): string {
    const o = { get g() { return arguments.length + ":" + typeof arguments; } };
    return o.g;
  }
}
console.log("generic", new Box<number>(1).peek(1, 2), new Box<string>("x").peek(3, 4));

function typeOf(a: number) {
  const o = { get g() { return typeof arguments; } };
  console.log("typeof", o.g);
}
typeOf(1);

show({ get g() { return arguments.length + ":" + typeof arguments; }, set s(v: number) {} });
const top = [{ get g() { return arguments.length; } }];
console.log("top elem", top[0].g);
