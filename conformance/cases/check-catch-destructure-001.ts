// P4.7 — `catch ({ field1, field2 }) { ... }` destructuring binding
// pattern (spec §13.15.5). Builds on the P4.7 throw substrate that
// widens `__torajs_throw_set(value)` into `__torajs_throw_set(tag,
// value)`, adds `__torajs_throw_take_tag()` to peek the dynamic tag
// without clearing active, and adds a 3rd throw global
// `__torajs_throw_tag`.
//
// Parser captures the destructure pattern's binding names + source
// keys; after parsing the catch body, prepends synthetic
// `const x: any = __catch_destr_NN.x;` lets so user-source references
// to the destructured names resolve normally.
//
// Catch slot `: any` is forced when the parser sees a destructure
// pattern; ssa_lower at the catch site reads (tag, value) from the
// throw globals and reconstructs an Any-box.
//
// Subset boundary: the throw value must lower as a DYNOBJ-backed Any
// (i.e., `let v: any = {...}; throw v`) — bare `throw {...}` lowers
// the ObjectLit as a typed Type::Obj(sid) whose heap layout differs
// from dynobj, and Member-on-Any reads at +16 expect dynobj layout.
// Auto-converting ObjectLit throws to dynobj would break typed
// `catch (e: T)` paths that depend on the typed-Obj layout (see
// throw-002-struct fixture). The clean substrate path is sid-aware
// Member-on-Any dispatch — deferred to a future substrate phase.

// 1. Multi-field destructure with Any-typed object throw.
function throwIt1(): void {
  const v: any = { code: 42, msg: "boom" };
  throw v;
}
try {
  throwIt1();
} catch ({code, msg}) {
  console.log(code);   // 42
  console.log(msg);    // boom
}

// 2. Single-field destructure.
function throwIt2(): void {
  const v: any = { name: "alice" };
  throw v;
}
try {
  throwIt2();
} catch ({name}) {
  console.log(name);   // alice
}

// 3. Aliased destructure: `{code: c}` binds to `c`.
function throwIt3(): void {
  const v: any = { code: 7 };
  throw v;
}
try {
  throwIt3();
} catch ({code: c}) {
  console.log(c);      // 7
}

// 4. Multiple aliased + non-aliased.
function throwIt4(): void {
  const v: any = { x: 1, y: 2, z: 3 };
  throw v;
}
try {
  throwIt4();
} catch ({x: a, y, z: c}) {
  console.log(a);      // 1
  console.log(y);      // 2
  console.log(c);      // 3
}

// 5. Regression: throw + catch typed string still works.
try {
  throw "hello";
} catch (e: string) {
  console.log(e);          // hello
  console.log(e.length);   // 5
}

// 6. Regression: throw number + catch any reads correctly.
try {
  throw 99;
} catch (e: any) {
  console.log(typeof e);   // number
  console.log(e === 99);   // true
}
