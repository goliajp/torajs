// RFC 20260725-objlit-computed-key — computed keys evaluate at
// runtime through the dynobj lane: ToPropertyKey string face, field
// order = evaluation order, method shorthand bodies are real.

// face 1 — string-variable key.
let k1 = "alpha";
let o1 = { [k1]: 1, plain: 2 };
console.log(o1.alpha);
console.log(o1.plain);

// face 2 — expression key + number key canonicalization.
let o2 = { ["a" + "b"]: 3, [40 + 2]: "num" };
console.log(o2.ab);
console.log(o2[42]);

// face 3 — toString side-effect order (to-name-side-effects shape).
let order: string[] = [];
let key1 = {
  toString() {
    order.push("key");
    return "kk";
  },
};
let o3 = { [key1]: 9 };
console.log(o3.kk);
console.log(order.length);

// face 4 — computed-key method shorthand with a real body.
let m = "greet";
let o4 = {
  [m]() {
    return "hello";
  },
};
console.log(o4.greet());
