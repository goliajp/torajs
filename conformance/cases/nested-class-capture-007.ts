// Two capturing classes sharing a name. Receiver promotion pairs a
// binding to its uses by NAME and refuses when the name is declared
// more than once program-wide, so the lowering gives each minted
// binding a name of its own.
function a1(a: number): number {
  class K {
    x: number;
    constructor() {
      this.x = a;
    }
    m(): number {
      return this.x;
    }
  }
  return new K().m();
}
function a2(b: number): number {
  class K {
    y: number;
    constructor() {
      this.y = b * 10;
    }
    n(): number {
      return this.y;
    }
  }
  return new K().n();
}
console.log(a1(1), a2(2));

// A block-scoped one shadowing a top-level class of the same name:
// the inner shadow takes the runtime-value lane, the outer keeps the
// static class machinery.
class C {
  m(): string {
    return "outer";
  }
}
{
  const v = "inner";
  class C {
    m(): string {
      return v;
    }
  }
  console.log(new C().m());
}
console.log(new C().m());
