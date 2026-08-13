// ES §13.10.2 — the right-hand side of `instanceof` is an expression,
// not a name. Every spelling here used to die in the parser.
class Base {
  x: number = 1;
}
class Derived extends Base {
  y: number = 2;
}

const d = new Derived();
const b = new Base();

// A class reached through a property — the target is a VALUE, so the
// compile-time tag fold has no name to key on and the runtime
// operator answers instead.
const box = { cls: Base };
console.log(d instanceof box.cls, b instanceof box.cls);

// Through a cast, and through a call.
const C: any = Derived;
console.log(d instanceof (C as any), b instanceof (C as any));

function pick(deep: boolean): any {
  return deep ? Derived : Base;
}
console.log(d instanceof pick(true), b instanceof pick(true));
console.log(b instanceof pick(false));

// The target expression closes over an outer binding — the walkers
// have to see `holder` as a reference now that the right-hand side is
// a real subtree.
const holder = {
  get(): any {
    return Base;
  },
};
console.log(d instanceof holder.get());

// A bound function's OrdinaryHasInstance follows the target it was
// bound from (§10.4.1.2).
function Ctor(this: any): void {}
const bound: any = (Ctor as any).bind(null);
const made: any = new (Ctor as any)();
console.log(made instanceof bound);

// Steps 1 and 4 throw rather than answering false.
try {
  console.log((d as any) instanceof (42 as any));
} catch (e: any) {
  console.log("not-object:", e instanceof TypeError);
}
try {
  console.log((d as any) instanceof (null as any));
} catch (e: any) {
  console.log("null:", e instanceof TypeError);
}
try {
  console.log((d as any) instanceof ({ plain: 1 } as any));
} catch (e: any) {
  console.log("not-callable:", e instanceof TypeError);
}

// A handler on the target still decides, reached through an
// expression rather than a name.
const withHandler: any = {};
Object.defineProperty(withHandler, Symbol.hasInstance, {
  value: (v: any) => typeof v === "number" && v % 2 === 0,
});
const wrap = { h: withHandler };
console.log((4 as any) instanceof wrap.h, (5 as any) instanceof wrap.h);

// The bare-name spellings keep the fold they always had.
console.log(d instanceof Base, d instanceof Derived, b instanceof Derived);
