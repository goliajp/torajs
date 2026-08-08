// SuperProperty in a class WITHOUT an extends clause (sec 10.2.4):
// the home object is C.prototype, so the super base is
// %Object.prototype% — inherited Object methods resolve, anything
// else reads undefined.
class A {
  m(): string {
    const f: any = super.toString;
    return typeof f;
  }
  n(): any {
    return super.missing;
  }
}
const a = new A();
console.log(a.m(), a.n());
