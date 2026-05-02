// Phase J.1 — `function*` generator declarations + linear yield
// sequences. tr's MVP scope:
//   - body is a flat list of statements; yields appear at the top
//     level (no yield inside if / while / for / try / switch yet).
//   - explicit return-type annotation supplies the yield value type.
//   - generator parameters are stored as fields on the iterator
//     instance and accessed via `this.<param>` in the rewritten body.
// desugar emits:
//   - `class __Gen_<name>` with `__state: number` + per-param fields,
//     ctor that stores params, `next(): { value: T, done: boolean }`
//     that switches on `__state`.
//   - factory FnDecl `<name>(args): __Gen_<name>` returning
//     `new __Gen_<name>(args)`.

function* count3(): number {
  yield 1;
  yield 2;
  yield 3;
}

function* pair(a: number, b: number): number {
  yield a;
  yield b;
  yield a + b;
}

// J.2.a — `let`s declared at the top level of a generator body get
// lifted to fields on the iterator class so the binding survives
// across yield boundaries.
function* with_locals(): number {
  let x = 10;
  yield x;
  let y = 20;
  yield x + y;
  yield x * y;
}

function check(): number {
  // Basic linear drive — three yields, fourth call returns done.
  let g = count3();
  let s1 = g.next();
  if (s1.value !== 1 || s1.done !== false) { throw "#1: first yield"; }
  let s2 = g.next();
  if (s2.value !== 2 || s2.done !== false) { throw "#2: second yield"; }
  let s3 = g.next();
  if (s3.value !== 3 || s3.done !== false) { throw "#3: third yield"; }
  let s4 = g.next();
  if (s4.done !== true) { throw "#4: exhausted"; }
  // Re-call after exhaustion stays done.
  let s5 = g.next();
  if (s5.done !== true) { throw "#5: re-exhausted"; }

  // Independent generator instances.
  let g1 = count3();
  let g2 = count3();
  if (g1.next().value !== 1) { throw "#6: g1-0"; }
  if (g1.next().value !== 2) { throw "#7: g1-1"; }
  if (g2.next().value !== 1) { throw "#8: g2 independent"; }
  if (g1.next().value !== 3) { throw "#9: g1-2"; }
  if (g2.next().value !== 2) { throw "#10: g2-1"; }

  // Generator with parameters — yields reference params via this.<name>.
  let p = pair(10, 20);
  if (p.next().value !== 10) { throw "#11: param a"; }
  if (p.next().value !== 20) { throw "#12: param b"; }
  if (p.next().value !== 30) { throw "#13: param sum"; }
  if (p.next().done !== true) { throw "#14: pair end"; }

  // Sum across a manual drain — protocol-driven loop.
  let total: number = 0;
  let it = count3();
  while (true) {
    let step = it.next();
    if (step.done) { break; }
    total = total + step.value;
  }
  if (total !== 6) { throw "#15: drain sum"; }

  // J.2.a — cross-yield locals lifted to fields.
  let h = with_locals();
  if (h.next().value !== 10) { throw "#16: x"; }
  if (h.next().value !== 30) { throw "#17: x+y"; }
  if (h.next().value !== 200) { throw "#18: x*y"; }
  if (h.next().done !== true) { throw "#19: locals end"; }

  return 0;
}
console.log(check());
