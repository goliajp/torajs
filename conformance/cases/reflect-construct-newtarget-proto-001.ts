// Reflect.construct: newTarget.prototype decides the new object's
// [[Prototype]]; a non-object one falls back to the constructor's
// intrinsic default (its own prototype for a builtin, Object.prototype
// under user classes).
function nt() {}
const ps: any[] = [undefined, null, true, "", Symbol(), 1, "abc", 5n];
class Foo { a = 1; }
class Bar extends Foo { b = 2; }
class MyErr extends Error {}
const line: string[] = [];
for (const p of ps) {
  (nt as any).prototype = p;
  const ds = Reflect.construct(DisposableStack, [], nt);
  const f = Reflect.construct(Foo, [], nt);
  const b = Reflect.construct(Bar, [], nt);
  const e = Reflect.construct(MyErr, ["m"], nt);
  const ae = Reflect.construct(AggregateError, [[]], nt);
  line.push([
    Object.getPrototypeOf(ds) === DisposableStack.prototype,
    Object.getPrototypeOf(f) === Object.prototype,
    Object.getPrototypeOf(b) === Object.prototype,
    Object.getPrototypeOf(e) === Error.prototype,
    Object.getPrototypeOf(ae) === AggregateError.prototype,
  ].join(" "));
}
console.log(line.join("\n"));

const custom = { x: 42 };
const newt = new Proxy(function () {}, {
  get(t: any, p: any) {
    return p === "prototype" ? custom : t[p];
  },
});
const agg = Reflect.construct(AggregateError, [[]], newt);
console.log(Object.getPrototypeOf(agg) === custom, (agg as any).x);
const st = Reflect.construct(DisposableStack, [], newt);
console.log(Object.getPrototypeOf(st) === custom, (st as any).x);

class O extends Object {}
const o2 = Reflect.construct(Object, [{ b: 2 }], O);
console.log(Object.getPrototypeOf(o2) === O.prototype, (o2 as any).b);

class Base {
  arg: number;
  constructor(arg: number) { this.arg = arg; }
}
class Sub extends Base {
  get isSub() { return true; }
}
const FooProxy = new Proxy(new Proxy(Base, {}), { construct: null as any });
const sub = Reflect.construct(FooProxy, [2], Sub);
console.log(sub instanceof Sub, (sub as any).arg, (sub as any).isSub);
