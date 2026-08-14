// Control half — a nested class that captures nothing keeps the
// hoist lane, including a capture-free `extends` between two of them.
function outer(): number {
  class Base {
    m(): number {
      return 1;
    }
  }
  class D extends Base {
    n(): number {
      return this.m() + 1;
    }
  }
  return new D().n();
}
console.log(outer());

{
  class K {
    m(): number {
      return 7;
    }
  }
  console.log(new K().m());
}
