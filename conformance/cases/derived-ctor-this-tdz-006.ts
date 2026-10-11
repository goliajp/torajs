// an arrow's parameter default reads the constructor's this binding:
// before super() it is in its temporal dead zone (ECMA-262 §9.1.1.3.4)
class A {
  tag = "a";
}
class B extends A {
  constructor(early: boolean) {
    const f = (x: any = this) => x;
    if (early) {
      try {
        f();
        console.log("early read: no throw");
      } catch (e) {
        console.log("early read:", e instanceof ReferenceError);
      }
    }
    super();
    console.log("late read:", f().tag);
  }
}
new B(true);
new B(false);
class D extends A {
  constructor() {
    const h = (y: number = 7, z: any = this) => y;
    super();
    console.log("after super:", h(), h(3));
  }
}
new D();
