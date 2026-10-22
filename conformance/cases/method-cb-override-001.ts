// An arrow handed to a method takes the parameter types the method
// declares, unless a subclass overrides the method: TS checks method
// parameters bivariantly, so `B.run` may call the arrow with a string
// while `A.run` declares a number.

class A {
  run(f: (x: number) => void): void {
    f(1.5);
  }
}
class B extends A {
  run(f: (x: any) => void): void {
    f("s");
  }
}
class C extends B {}
const a: A = new B();
a.run((x) => console.log("a", typeof x, x));
const c: A = new C();
c.run((x) => console.log("c", typeof x, x));
const plain: A = new A();
plain.run((x) => console.log("plain", typeof x, x));
