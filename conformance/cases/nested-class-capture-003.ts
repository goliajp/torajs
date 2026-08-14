// The constructor body captures too, instance fields ride it, and
// one method calls another through `this`.
function outer(a: number): number {
  class K {
    x: number;
    constructor(p: number) {
      this.x = a + p;
    }
    m(): number {
      return this.x;
    }
    n(): number {
      return this.m() * 2;
    }
  }
  return new K(3).n();
}
console.log(outer(7));
