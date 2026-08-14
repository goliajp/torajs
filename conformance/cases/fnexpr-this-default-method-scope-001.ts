// 399-04 (certain_bindings / alias census) — a `const` written once
// inside a method is ONE declaration: the receiver-polymorphic twin
// clone is a copy, not a re-declared name. §27.2.5.4 calls a then
// handler with NO receiver, so `typeof this` is "undefined" whether
// the program sits at top level or inside a method body.

// inline handler over a const-bound promise, at method scope
class M {
  m() {
    const p = Promise.resolve(1);
    return p.then(function () {
      console.log(typeof this);
    });
  }
}
new M().m();

// the handler reached through a const NAME (the alias census) —
// names are distinct per class: the census is program-wide by NAME
// (the deliberately coarse 399-03 bar)
class W {
  m() {
    const pw = Promise.resolve(1);
    const fw = function () {
      console.log(typeof this);
    };
    return pw.then(fw);
  }
}
new W().m();

// the same alias shape through the any lane runs the twin clone
class V {
  m() {
    const pv = Promise.resolve(1);
    const fv = function () {
      console.log(typeof this);
    };
    return pv.then(fv);
  }
}
const v: any = new V();
v.m();

// top level keeps its answer
const p0 = Promise.resolve(1);
p0.then(function () {
  console.log(typeof this);
});
