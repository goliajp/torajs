// class C extends Function — exotic-backed callable instances (RFC
// 20260730 blade 2). The instance is a REAL Tag::Closure cell:
// typeof "function", instanceof Function via tag-eq, callable as the
// empty function (§20.2.1.1 with no body source); class identity
// rides FLAG_SUBCLASSED + the blade-0 side table.

// 1. default ctor, builtin faces
class MyFn extends Function {}
const f = new MyFn();
console.log(typeof f);
console.log(f instanceof Function, f instanceof MyFn);
console.log(Object.getPrototypeOf(f) === MyFn.prototype);
console.log((f as any)());

// 2. explicit ctor with bare super()
class Tagged extends Function {
  constructor() {
    super();
  }
  tag(): string {
    return "T";
  }
}
const t = new Tagged();
console.log(t.tag(), typeof t, t instanceof Tagged);

// 3. plain functions keep their answers (and never read the side table)
function g(): number {
  return 1;
}
console.log(g instanceof MyFn, g());
const arrow = (x: number): number => x + 1;
console.log(arrow instanceof Tagged, arrow(41));
