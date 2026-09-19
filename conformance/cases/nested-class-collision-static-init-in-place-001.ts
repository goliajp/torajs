// A nested class whose name another function's class shares is
// renamed by the hoist; when a sibling class body names it, it also
// takes the capturing lane. Its static initializers still run where
// the class is written, and `.name` / `String(C)` keep the source
// spelling and text.
//
// `.expected` rather than a live bun diff: bun 1.4.2 answers `""` for
// `Kk.name` below; node v26 and §8.4.5 NamedEvaluation answer `K`.
const log: string[] = [];
function mark(tag: string): number {
  log.push(tag);
  return log.length;
}
function f(): any {
  log.push("f-start");
  class A {
    static x = mark("f.A.x");
  }
  class B {
    m(): any { return new A(); }
  }
  log.push("f-end");
  return [A, B];
}
function g(): any {
  log.push("g-start");
  class A {
    static y = mark("g.A.y");
  }
  log.push("g-end");
  return A;
}
log.push("top");
const pf = f();
const Ag = g();
console.log(log.join(","));
console.log(pf[0].name, Ag.name, pf[0].x, Ag.y, pf[0] === Ag);
console.log(new pf[1]().m() instanceof pf[0], String(pf[0]).slice(0, 7), String(Ag).slice(0, 7));

// the same lane without a collision
function h(): any {
  class C {
    static z = mark("h.C.z");
  }
  class D {
    m(): any { return new C(); }
  }
  return [C, D];
}
const ph = h();
console.log(ph[0].name, ph[0].z, String(ph[0]).slice(0, 7), new ph[1]().m() instanceof ph[0]);

// a class expression that captures a local
function k(n: number): any {
  const K = class {
    static w = mark("k.K.w");
    get(): number { return n; }
  };
  return K;
}
const Kk = k(7);
console.log(Kk.name, Kk.w, new Kk().get(), String(Kk).slice(0, 5));
console.log(log.join(","));
