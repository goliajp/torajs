// RFC 20260828 — a top-level fn name that a local binding shadows is
// still a capture. The lift pre-binds every top-level fn name so a
// reference to one does not become a capture; when a param or a `let`
// rebinds that name the reference belongs to the local, and the flat
// arena scan could not see which body the arrow sat in. `typeof g` read
// `object` outside the arrow and `function` inside it, in the same call.
function g(): number {
  return 99;
}

// param shadows
function viaParam(g: any): void {
  const f: any = () => {
    console.log("param", typeof g, g);
  };
  f();
}
viaParam(5);

// local let shadows
function viaLet(): void {
  const g: any = 7;
  const f: any = () => {
    console.log("let", typeof g, g);
  };
  f();
}
viaLet();

// two arrows deep
function viaNested(g: any): void {
  const f: any = () => {
    const h: any = () => {
      console.log("nested", typeof g, g);
    };
    h();
  };
  f();
}
viaNested(11);

// a class instance in the shadowed binding — this shape failed at the
// checker (`expected ClassRef("C"), got Function([], Number)`), not at
// run time
class C {
  m(): number {
    return 3;
  }
}
function viaClass(g: any): void {
  const f: any = () => {
    console.log("class", typeof g, g.m());
  };
  f();
}
viaClass(new C());

// the shape this was found through: a generator handle used inside an
// arrow that captures it, with a same-named top-level function present.
// (The variant where the shadowed name belongs to a top-level
// GENERATOR is a second defect — the method-call rewrite reads the
// same name-keyed table — and is filed as 521-03, not covered here.)
function* second() {
  yield 2;
}
function viaThen(g: any): void {
  const w: any = Promise.resolve(0);
  w.then((x: any) => {
    console.log("gen", g.next().value);
    return 0;
  });
}
viaThen(second());

// negative — an arrow calling a genuine, unshadowed top-level function
// must still resolve it without capturing (dropping the whole pre-bind
// set turns this into `closure capture 'top' not in scope`)
function top(): number {
  return 42;
}
const callTop: any = () => {
  console.log("top", top());
};
callTop();
