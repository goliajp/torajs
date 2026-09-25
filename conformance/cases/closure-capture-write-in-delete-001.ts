// a closure that writes a captured binding inside a `delete` operand or
// a computed `new` callee still shares that binding with its frame.
class A {
  v: number;
  constructor() {
    this.v = 7;
  }
}
function viaDelete(): number {
  let x = 0;
  const o: any = { a: 1 };
  const f = (): void => {
    delete o[((x = 5), "a")];
  };
  f();
  return x + ("a" in o ? 100 : 0);
}
function viaNew(): number {
  let y = 0;
  const make = (): A => new ((y = 3) > 0 ? A : A)();
  return make().v + y;
}
class Base {}
class Derived extends Base {
  probe(): number {
    let side = 0;
    try {
      const g = (): boolean => delete super[(side = 1)];
      g();
    } catch (e) {
      side = side + 10;
    }
    return side;
  }
}
console.log(viaDelete(), viaNew(), new Derived().probe());
