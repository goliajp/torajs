// An async method's body moves into a generator whose receiver is the
// leading `__genrecv` parameter; the `this` of a function expression
// declared INSIDE that body is that expression's own and must not be
// rewritten to the method's receiver (it answered the outer object's
// field). An arrow's `this` stays the method's.
class C {
  z = 10;
  async m(): Promise<any> {
    await null;
    const inner = function (this: any) { return this.z; };
    const arrow = () => this.z;
    return inner.call({ z: 1 }) + arrow() + this.z;
  }
  async n(): Promise<any> {
    const o = { z: 100, get(): number { return this.z; } };
    await null;
    return o.get() + this.z;
  }
}
const c = new C();
c.m().then((x: any) => console.log("m", x));
c.n().then((x: any) => console.log("n", x));
