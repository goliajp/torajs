// A class member or closure in a generator body reads the generator's
// local only where no scope inside it declares the name again: a
// declaration list, a nested block, a `for (let …)` head, a catch
// parameter or a `var` all bind their own name (§14.2.2, §14.7.4,
// §14.15.2, §10.2.11). A class's computed keys and static
// initializers read the generator's locals as well (§15.7.14); a
// heritage that names a class declared in the generator stays that
// class.

function* multiDecl() {
  let p = 1;
  class C {
    m() {
      let a = 1,
        p = 7;
      return a + p;
    }
  }
  yield new C().m() + p;
}
for (const v of multiDecl()) console.log(v);

function* nestedBlock() {
  let p = 1;
  class C {
    m() {
      const r = p;
      {
        let p = 7;
        return r + p;
      }
    }
  }
  yield new C().m();
}
for (const v of nestedBlock()) console.log(v);

function* catchParam() {
  let p = 1;
  class C {
    m() {
      let s = 0;
      try {
        throw 5;
      } catch (p) {
        s = p;
      }
      return s * 10 + p;
    }
  }
  yield new C().m();
}
for (const v of catchParam()) console.log(v);

function* varInBlock() {
  let q = 1;
  class C {
    m() {
      {
        var q = 9;
      }
      return q;
    }
  }
  yield new C().m() + q;
}
for (const v of varInBlock()) console.log(v);

function* forLetInMember() {
  let n = 100;
  class C {
    m() {
      let s = 0;
      for (let n = 0; n < 3; n++) s += n;
      return s + n;
    }
  }
  yield new C().m();
}
for (const v of forLetInMember()) console.log(v);

function* forLetInMemberLoop() {
  const fs: (() => number)[] = [];
  let c = 0;
  while (c < 3) {
    let n = c * 10;
    class C {
      m() {
        let s = 0;
        for (let n = 0; n < 2; n++) s += n;
        return s + n;
      }
    }
    fs.push(() => new C().m());
    c++;
    yield 0;
  }
  for (const f of fs) console.log(f());
}
for (const _ of forLetInMemberLoop()) {}

function* arrowMulti() {
  let p = 1;
  const f = () => {
    let a = 2,
      p = 7;
    return a + p;
  };
  yield f() + p;
}
for (const v of arrowMulti()) console.log(v);

function* nestedClassMulti() {
  let p = 1;
  class C {
    m() {
      class D {
        k() {
          let a = 3,
            p = 4;
          return a * p;
        }
      }
      return new D().k() + p;
    }
  }
  yield new C().m();
}
for (const v of nestedClassMulti()) console.log(v);

function* extendsLocal() {
  class A {
    v() {
      return 5;
    }
  }
  let k = 3;
  yield 0;
  class D extends A {
    w() {
      let k = 10,
        j = 1;
      return this.v() + k + j;
    }
  }
  yield new D().w() + k;
}
for (const v of extendsLocal()) console.log(v);

function* staticAndDefault() {
  let p = 2;
  yield 0;
  class C {
    static s = p * 10;
    static t = 0;
    static {
      let p = 99;
      C.t = p;
    }
    m(q = p) {
      for (const p of [5]) q += p;
      return q;
    }
  }
  yield C.s;
  yield C.t;
  yield new C().m();
}
for (const v of staticAndDefault()) console.log(v);

function* computedKeyLocal() {
  let key = "hello";
  yield 0;
  class C {
    [key]() {
      return 42;
    }
  }
  const o: any = new C();
  yield o.hello();
}
for (const v of computedKeyLocal()) console.log(v);
