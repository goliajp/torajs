// RFC 20260828 knife 2 — the fn-VALUE rewrite half. `lift_arrow_fns`
// runs before the forwarder collector, so a lifted closure's captures
// are neither its params nor its body locals — they ride the env — and
// the collector's shadow stack (rotation 128) could not see them. A
// captured name spelling a top-level fn was therefore rewritten to
// `__forward_<name>`, and the receiver became the top-level function
// instead of the captured value: `typeof g.next` answered `function`
// while `g.next()` threw "not a function", in the same program.
//
// After knife 1 a top-level fn name only lands in a capture list when
// some enclosing scope rebound it, so a capture spelling one is
// precisely the shadowed case.
function* g() {
  yield 7;
}
function* second() {
  yield 2;
}

// arrow captures a generator handle shadowing the generator's name
function viaArrow(g: any): void {
  const f: any = () => {
    console.log("arrow", g.next().value);
  };
  f();
}
viaArrow(second());

// the same through a `.then` handler
function viaThen(g: any): void {
  const w: any = Promise.resolve(0);
  w.then((x: any) => {
    console.log("then", g.next().value);
    return 0;
  });
}
viaThen(second());

// and through a `new Promise` executor
function viaExecutor(g: any): any {
  return new Promise((resolve: any, reject: any) => {
    console.log("executor", g.next().value);
    resolve(0);
  });
}
viaExecutor(second());

// the reads and the calls must agree
function viaTypeof(g: any): void {
  const f: any = () => {
    console.log("agree", typeof g, typeof g.next, g.next().value);
  };
  f();
}
viaTypeof(second());

// negative — an arrow using a genuine unshadowed top-level generator
// factory still reaches the factory
function viaFactory(): void {
  const f: any = () => {
    console.log("factory", g().next().value);
  };
  f();
}
viaFactory();
