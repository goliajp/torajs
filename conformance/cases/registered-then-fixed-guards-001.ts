// Nine registered defects turned out to be already fixed when this
// rotation re-verified them, which means they were closed by changes
// that never pinned the behaviour. These lines are that pin — each
// one is a shape the registry described as wrong and the tree now
// answers right.
const zs: number[] = [1, 2, 3];

// r687 ② — a declared `{ k: any }` slot reads back what was stored.
const r = zs[9];
const boxed: { k: any } = { k: r };
console.log(boxed.k, typeof boxed.k, boxed.k === undefined);

// r689 ⑥ — RegExp.lastIndex keeps the value it was handed.
const re: any = /a/g;
const keeper = {
  valueOf() {
    return 2;
  },
};
re.lastIndex = keeper;
console.log(typeof re.lastIndex, re.lastIndex === keeper);

// r689 ⑨ — the callable coercion forms still evaluate their extra
// arguments (§21.1.1.1 evaluates the argument list, then takes the
// first).
function loud(): number {
  console.log("evaluated");
  return 0;
}
console.log(Number(1, loud() as any), String("a", loud() as any));

// r692 ㉖ — a method named `size` on an Array subclass is reachable.
class Sized extends Array {
  size(): number {
    return this.length;
  }
}
const sized: any = new Sized();
sized.push(1, 2);
console.log(typeof sized.size, sized.size());

// r692 ㉗ — a Number subclass instance reaches Number.prototype, and
// the wrapper's method is the prototype's own cell.
class Sub extends Number {}
console.log(typeof (new Sub(3) as any).toPrecision);
console.log((new Number(3) as any).toPrecision === Number.prototype.toPrecision);

// r692 ㉙ — an accessor's setter has length 1 (§10.2.9).
const holder: any = {};
Object.defineProperty(holder, "k", {
  set(_v: any) {},
  configurable: true,
});
const desc = Object.getOwnPropertyDescriptor(holder, "k");
console.log(desc && desc.set ? desc.set.length : -1);

// r690 ⑰ — Promise.all of a non-iterable rejects with a TypeError.
Promise.all(5 as any)
  .then(() => console.log("unexpected"))
  .catch((e) => console.log("rejected", e instanceof TypeError));
