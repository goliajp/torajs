// Throw helper msg literal alignment with bun. Spec leaves TypeError msg
// wording implementation-defined; this fixture pins every site we own to
// bun's exact wording so user-side `catch (e) { e.message }` assertions
// (incl. test262 style) round-trip cross-runtime. Covers:
//   * Object.defineProperty target Type(O) check (reflect.rs)
//   * dict redefine — configurable / enumerable / writable / value
//     mismatches (dynobj/define.rs)
//   * dict set — getter-only accessor + non-writable data property
//     (dynobj/set.rs)
//   * Object.freeze + assign (torajs-rc/freeze.rs)

// reflect.rs:436 — Object.defineProperty(null) → "Properties can only be defined on Objects."
try {
  Object.defineProperty(null, "x", { value: 1 });
} catch (e) {
  console.log(e.message);
}

// reflect.rs:436 — Object.defineProperty(42) → same msg.
try {
  Object.defineProperty(42, "x", { value: 1 });
} catch (e) {
  console.log(e.message);
}

// dynobj/define.rs:124 — redefine configurable on non-configurable entry.
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    value: 1,
    configurable: false,
  });
  Object.defineProperty(o, "x", {
    value: 2,
    configurable: true,
  });
} catch (e) {
  console.log(e.message);
}

// dynobj/define.rs:133 — redefine enumerable on non-configurable entry.
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    value: 1,
    enumerable: false,
    configurable: false,
  });
  Object.defineProperty(o, "x", {
    value: 2,
    enumerable: true,
  });
} catch (e) {
  console.log(e.message);
}

// dynobj/define.rs:143 — redefine writable from false to true on
// non-configurable entry.
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    value: 1,
    writable: false,
    configurable: false,
  });
  Object.defineProperty(o, "x", {
    value: 1,
    writable: true,
  });
} catch (e) {
  console.log(e.message);
}

// dynobj/define.rs:158 — redefine value while writable=false (value mismatch).
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    value: 1,
    writable: false,
    configurable: false,
  });
  Object.defineProperty(o, "x", { value: 2 });
} catch (e) {
  console.log(e.message);
}

// dynobj/set.rs:92 — assignment through a getter-only accessor pair.
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    get() {
      return 1;
    },
  });
  o.x = 2;
} catch (e) {
  console.log(e.message);
}

// dynobj/set.rs:102 — assignment to a non-writable data property.
try {
  const o: any = {};
  Object.defineProperty(o, "x", {
    value: 1,
    writable: false,
  });
  o.x = 2;
} catch (e) {
  console.log(e.message);
}

// torajs-rc/freeze.rs:99 — assignment after Object.freeze (typed class
// instance path through the rc-side `__torajs_obj_check_not_frozen`).
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
try {
  const b = new Box(1);
  Object.freeze(b);
  b.v = 2;
} catch (e) {
  console.log(e.message);
}
