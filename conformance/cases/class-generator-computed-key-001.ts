// §15.4 ClassElementName → ComputedPropertyName on a generator
// method. The class half of the generator parser had no computed-name
// branch at all, so every bracket spelling — the folded
// `Symbol.iterator`, a literal key, and a runtime key — died at
// "expected generator method name after `*`".
const k = "step";

class Range {
  *[Symbol.iterator]() {
    yield 1;
    yield 2;
  }
}
console.log([...new Range()].join(","));

class Runtime {
  *[k](a: number) {
    yield a;
    yield a + 1;
  }
}
console.log([...(new Runtime() as any)[k](5)].join(","));

class Statics {
  static *[k]() {
    yield 7;
  }
}
console.log([...(Statics as any)[k]()].join(","));

class Literal {
  *["lit"]() {
    yield 3;
  }
}
console.log([...(new Literal() as any).lit()].join(","));
