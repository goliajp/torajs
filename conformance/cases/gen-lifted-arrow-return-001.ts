// A generator local holding an arrow whose return type nothing here
// can read, and a local holding what calling one answers.
//
// The lift reads an arrow's shape straight out of the node, but its
// return type still goes through the seeded sniff — and that sniff has
// no arm for an arrow in return position. `const add = (n: number) =>
// (m: number) => n + m` therefore declined, and declining handed the
// field back to the `number` fallback, which is a claim about the
// return rather than an absence of one. `any` is the honest answer,
// and an explicit `: any` on the same two lines already worked.
//
// The second half is the call. `fn_sigs` is keyed on top-level
// function names, so a local holding a closure was not in it and
// `const add3 = add(3)` fell to `number` too — after which `add3(4)`
// said "not callable: type Number". The local's own annotation is
// fn-shaped and says what calling it answers.
//
// Arrows whose return the sniff CAN read keep the precise type they
// had: `double` below is still `__cls(number)->number`, which is what
// lets `doubled` be a number the arithmetic below can use.

function* g(): number {
  // return type unreadable — an arrow returning an arrow
  const add = (n: number) => (m: number) => n + m;
  const add3 = add(3);
  yield add3(4);

  // and one more call of the same curried local, so the annotation
  // has to survive being read twice
  const add10 = add(10);
  yield add10(5);

  // return type readable — unchanged, and still arithmetic-typed
  const double = (n: number) => n * 2;
  const doubled = double(21);
  yield doubled + 0;

  // an arrow reading the generator's own earlier locals still works
  const base = 100;
  const offset = (n: number) => n + base;
  yield offset(7);
}

for (const v of g()) {
  console.log(v);
}
