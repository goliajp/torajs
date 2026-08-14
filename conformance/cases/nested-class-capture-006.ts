// Static methods on a class that captures. They hang off the
// constructor rather than the prototype, so `K.s()` reaches them and
// `new K()` does not.
function outer(a: number): number {
  class K {
    static s(): number {
      return a * 2;
    }
    static twice(): number {
      return K.s() + K.s();
    }
  }
  return K.twice();
}
console.log(outer(7));

function both(a: number): string {
  class J {
    x: number;
    constructor() {
      this.x = a;
    }
    m(): number {
      return this.x + 1;
    }
    static id(): string {
      return "J";
    }
  }
  return J.id() + String(new J().m());
}
console.log(both(4));
