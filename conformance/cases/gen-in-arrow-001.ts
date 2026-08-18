// 393-03 follow-up — a `function*` declaration inside an ARROW body
// lives in the expr arena, invisible to the stmt-spine lift walk, so
// it kept its `yield` and died loud at check ("yield is only valid
// inside a `function*` generator body"). The lift now claims arrow
// bodies through a flat arena scan (every arrow owns an arena slot,
// so one pass covers any nesting depth).
const f = () => {
  function* g() {
    yield 1;
    yield 2;
  }
  const it = g();
  console.log(typeof it);
  console.log(JSON.stringify(it.next()));
  console.log(JSON.stringify(it.next()));
  console.log(JSON.stringify(it.next()));
};
f();
// t262 GeneratorPrototype not-a-constructor shape: gen.next as a
// `new` target throws (built-ins without [[Construct]]).
const h = () => {
  function* g2() {}
  let iterator = g2();
  new iterator.next();
};
try {
  h();
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
// nested arrow-in-arrow — each depth owns its own arena slot.
const outer = () => {
  const inner = () => {
    function* g3() {
      yield "deep";
    }
    return g3().next().value;
  };
  return inner();
};
console.log(outer());
