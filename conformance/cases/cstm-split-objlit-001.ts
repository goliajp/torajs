// §22.1.3.23 step 2 — a computed-key object literal plants a user
// `@@split` (`{ [Symbol.split]: fn }`), and the typed-receiver lane's
// store-evidence gate must see the literal spelling (r290; the
// index-assign spelling is cstm-split-001).
const sep: any = {
  [Symbol.split]: function (str: any, limit: any) {
    return [str, limit === undefined];
  },
};
console.log(JSON.stringify("abc".split(sep)));
// The limit passes through RAW (step 2 precedes step 4's ToUint32).
const sep2: any = {
  [Symbol.split]: function (_s: any, l: any) {
    return [l];
  },
};
console.log(JSON.stringify("abc".split(sep2, 7)));

// The literal pattern joins inline — no binding at all.
console.log(
  JSON.stringify(
    "xy".split({
      [Symbol.split]: function (s: any) {
        return [s, s];
      },
    } as any),
  ),
);

// @@match through the same literal spelling.
const pat: any = {
  [Symbol.match]: function (s: any) {
    return [s];
  },
};
console.log(JSON.stringify("xy".match(pat)));

// A plain-string separator in the same program keeps the split
// semantics through the probe-miss fallback.
const plain: any = ",";
console.log(JSON.stringify("a,b".split(plain)));

// any-receiver lane over the literal spelling.
const s: any = "qq";
console.log(JSON.stringify(s.split(sep)));
