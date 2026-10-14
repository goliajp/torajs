// A class declared in a closure, in a member of another class, or
// read by a closure made on a later pass of a generator loop keeps the
// bindings of the pass that made it (§14.2.2, §15.7.14), and a member's
// parameter default reads the generator's locals.

function* inArrow() {
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 3) {
    const n = c * 10;
    fs.push(() => {
      class C {
        m() {
          return n;
        }
      }
      return new C().m();
    });
    c++;
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of inArrow()) {}

function* inArrowForLet() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    fs.push(() => {
      class C {
        m() {
          return i;
        }
      }
      return new C().m();
    });
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of inArrowForLet()) {}

function* nested() {
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 3) {
    const n = c * 10;
    class C {
      m() {
        class D {
          k() {
            return n + 1;
          }
        }
        return new D().k();
      }
    }
    fs.push(() => new C().m());
    c++;
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of nested()) {}

function* nestedForLet() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    class C {
      m() {
        class D {
          k() {
            return i + 2;
          }
        }
        return new D().k();
      }
    }
    fs.push(() => new C().m());
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of nestedForLet()) {}

function* defaults(p: number) {
  const v = p + 1;
  class C {
    m(x: number = v) {
      return x * 2;
    }
  }
  yield new C().m();
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 3) {
    const n = c * 10;
    class D {
      m(x: number = n) {
        return x + 3;
      }
    }
    fs.push(() => new D().m());
    c++;
    yield 0;
  }
  for (let i = 0; i < 3; i++) {
    fs.push(() => {
      class E {
        m(y: number = i) {
          return y + 100;
        }
      }
      return new E().m();
    });
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const x of defaults(4)) if (x) console.log(x);

function* laterClosure() {
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 3) {
    const n = c * 10;
    class C {
      static s = n + 5;
      m() {
        return n;
      }
    }
    fs.push(() => new C().m() + C.s);
    c++;
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of laterClosure()) {}

function* shadowed() {
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 2) {
    const n = c;
    fs.push(() => {
      const n = 7;
      class C {
        m() {
          return n;
        }
      }
      return new C().m();
    });
    fs.push(() => {
      class C {
        m(n: number) {
          return n;
        }
      }
      return new C().m(9);
    });
    c++;
    yield n;
  }
  for (const f of fs) console.log(f());
}
for (const _ of shadowed()) {}

function* laterClosureForLet() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    class C {
      m() {
        return i * 2;
      }
    }
    fs.push(() => new C().m());
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of laterClosureForLet()) {}
