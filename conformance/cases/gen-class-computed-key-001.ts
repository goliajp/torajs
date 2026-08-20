// Generator-body class with computed member keys (the t262
// cpn-class-decl-*-yield family): the key expr lives in the
// class_computed_keys side table, so the lifted-local rewrite must
// walk it there (a `[k]` local read / a hoisted `[yield N]` temp
// becomes a state-machine field read); the hoist admit pins a
// this-reading key to its declaring scope; and a `new C()` of a
// generator-local class annotates the lifted field `any` (the class
// never reaches the top-level class index).
function* g1() {
  const k = 'm';
  class C {
    [k]() {
      return 8;
    }
  }
  let c = new C();
  yield 1;
  console.log(c[k]());
}
const i1 = g1();
i1.next();
i1.next();

function* g2() {
  class D {
    [yield 9]() {
      return 7;
    }
  }
  let d = new D();
  console.log(d['m']());
}
const i2 = g2();
i2.next();
i2.next('m');
