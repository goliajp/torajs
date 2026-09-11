// §28.1.1 Reflect.apply reads three arguments, and a missing one is
// `undefined` — a non-callable target is step 1's TypeError, a
// nullish argumentsList step 2's. The lowering used to claim the call
// only at three arguments or more, so `Reflect.apply(x)` fell through
// to the generic member-call lane, which refuses the shape
// ("unsupported member call shape: apply") instead of running the
// program bun runs.
//
// The `as any` casts are there because TypeScript's own signature
// requires the arguments this case deliberately omits; the runtime
// question is what happens when a program omits them anyway.
const R = Reflect as any;

try {
  R.apply(1);
  console.log("no-args-target", "no throw");
} catch (e) {
  console.log("no-args-target", (e as any).constructor.name);
}

try {
  R.apply(function () {
    return 1;
  });
  console.log("missing-list", "no throw");
} catch (e) {
  console.log("missing-list", (e as any).constructor.name);
}

try {
  R.apply(function () {
    return 1;
  }, null);
  console.log("two-args", "no throw");
} catch (e) {
  console.log("two-args", (e as any).constructor.name);
}

try {
  R.apply();
  console.log("no-args", "no throw");
} catch (e) {
  console.log("no-args", (e as any).constructor.name);
}

// The full spelling keeps working, receiver included.
const o = { v: 5 };
console.log("three", Reflect.apply(function () {
  return (this as any).v;
}, o, []));
