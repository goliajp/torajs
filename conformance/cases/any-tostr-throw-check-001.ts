// 0-check audit (rotation 130 L3b) — SSA lanes that run the runtime
// ToString (OrdinaryToPrimitive, user hooks observable) without a
// pending-throw check leaked a throwing toString past the
// expression as a later uncaught error. Covers the bare-globals /
// Number-namespace / default-sort / HTML-wrapper lanes; the
// return-coercion lane is tr-typed-tier-only (bun erases the
// annotation and never coerces) so it has no bun-parity face.

const boom: any = {
  toString() { throw new TypeError("boom"); },
  valueOf() { throw new TypeError("boomV"); },
};

function expectCaught(tag: string, run: () => void): void {
  try {
    run();
    console.log(tag, "no throw");
  } catch (e) {
    console.log(tag, e instanceof TypeError);
  }
}

// parseInt / parseFloat (bare globals)
expectCaught("parseInt:", () => {
  parseInt(boom);
});
expectCaught("parseFloat:", () => {
  parseFloat(boom);
});

// Number namespace twin (string-annotated face, Any at runtime).
// Number.parseInt(as-string) rides a different lane that never
// materializes the cast (recorded aside) — parseFloat covers the
// namespace ToString station.
expectCaught("num-parseFloat:", () => {
  Number.parseFloat(boom as string);
});

// default sort compare over Any elements
expectCaught("sort:", () => {
  const a: any[] = [boom, boom];
  a.sort();
});

// String.prototype HTML wrapper arg
expectCaught("anchor:", () => {
  "x".anchor(boom);
});

// non-throwing objects keep working on the same lanes
const ok: any = { toString() { return "42"; } };
console.log(parseInt(ok)); // 42
console.log("x".anchor(ok).length > 0); // true
console.log("done");
