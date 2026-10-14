// A class declared in a generator body reads the generator's locals
// and parameters from its members, and one declared in a loop body
// keeps the block bindings of the pass that made it (§15.7.14).

function* plain(p: number) {
  const v = 5;
  let w = 1;
  class C {
    x: number;
    constructor() {
      this.x = v + p;
    }
    m() {
      return v + w + this.x;
    }
    get g() {
      return v * 2;
    }
    static s() {
      return p;
    }
    shadow(v: number) {
      return v;
    }
  }
  yield 1;
  w = 100;
  const c = new C();
  console.log(c.m(), c.g, C.s(), c.shadow(9));
}
for (const _ of plain(4)) {}

function* samePass() {
  let n = 0;
  while (n < 2) {
    const v = n * 10;
    class C {
      m() {
        return v;
      }
    }
    yield 1;
    console.log(new C().m());
    n++;
  }
}
for (const _ of samePass()) {}

function* laterPass() {
  let n = 0;
  const cs: any[] = [];
  while (n < 2) {
    const v = n * 10;
    class C {
      m() {
        return v;
      }
    }
    yield 1;
    cs.push(new C());
    n++;
  }
  for (const c of cs) console.log(c.m());
}
for (const _ of laterPass()) {}

function* forLet() {
  const cs: any[] = [];
  for (let i = 0; i < 3; i++) {
    class C {
      m() {
        return i;
      }
    }
    cs.push(new C());
    yield i;
  }
  for (const c of cs) console.log(c.m());
}
for (const _ of forLet()) {}

function* assigned() {
  let n = 0;
  const cs: any[] = [];
  while (n < 2) {
    let u = n;
    class C {
      m() {
        return u;
      }
    }
    yield 1;
    u += 100;
    cs.push(new C());
    n++;
  }
  for (const c of cs) console.log(c.m());
}
for (const _ of assigned()) {}

function* memberWrites() {
  let n = 0;
  const cs: any[] = [];
  while (n < 2) {
    let u = n;
    class C {
      bump() {
        u += 1;
        return u;
      }
    }
    yield 1;
    cs.push(new C());
    n++;
  }
  for (const c of cs) console.log(c.bump(), c.bump());
}
for (const _ of memberWrites()) {}

function* classExpr() {
  let n = 0;
  const cs: any[] = [];
  while (n < 2) {
    const v = n * 10;
    const C = class {
      static s = v + 1;
      m(v: number) {
        return v;
      }
      n() {
        return v;
      }
    };
    yield 1;
    cs.push(C);
    n++;
  }
  for (const c of cs) console.log(c.s, new c().m(7), new c().n());
}
for (const _ of classExpr()) {}
