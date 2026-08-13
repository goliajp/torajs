// ES §25.5.2.2 step 3 — Call(replacerFunction, holder, «key, value»).
// Before this the second argument was lowered and DROPPED, so every
// line below answered the unmodified serialization and looked like a
// pass (the output was valid JSON either way).
const obj = { a: 1, b: 2 };
console.log(JSON.stringify(obj, function (k: string, v: any) {
  return typeof v === "number" ? v * 100 : v;
}));

// Array elements are visited under their index as the key.
console.log(JSON.stringify([1, 2], function (k: string, v: any) {
  return typeof v === "number" ? v + 1 : v;
}));
console.log(JSON.stringify([[1], [2]], function (k: string, v: any) {
  return typeof v === "number" ? v * 10 : v;
}));

// §25.5.2 step 11 — the root is visited first, under the key "".
const seen: string[] = [];
JSON.stringify({ x: { y: 1 } }, function (k: string, v: any) {
  seen.push("[" + k + "]");
  return v;
});
console.log(seen.join(","));

// A primitive root goes through the same wrapper.
console.log(JSON.stringify(5, function (k: string, v: any) {
  return typeof v === "number" ? v + 1 : v;
}));

// §25.5.2.2 step 2 runs BEFORE step 3: a Date reaches the replacer as
// its toJSON result, not as the Date.
console.log(JSON.stringify({ d: new Date(0) }, function (k: string, v: any) {
  return typeof v === "string" ? "S:" + v : v;
}));

// The struct lane (a class instance) walks its declared fields.
class Point {
  x: number = 1;
  y: number = 2;
}
console.log(JSON.stringify(new Point(), function (k: string, v: any) {
  return k === "y" ? 99 : v;
}));

// A replacer arriving as `any` is tested for callability at run time —
// this used to be the residual silent-drop hole.
const anyRep: any = function (k: string, v: any) {
  return typeof v === "number" ? v * 2 : v;
};
console.log(JSON.stringify({ n: 5 }, anyRep));

// §25.5.2 step 4 only consults an Object, so a non-callable in that
// slot is ignored — the spec's own answer, not a shortcut.
const notFn: any = "nope";
console.log(JSON.stringify({ n: 5 }, notFn));

// space still applies alongside a replacer.
console.log(JSON.stringify({ a: { b: 2 } }, function (k: string, v: any) {
  return typeof v === "number" ? v * 3 : v;
}, 1));

// A throw from the replacer body unwinds the walk.
try {
  JSON.stringify({ a: 1 }, function (k: string, v: any) {
    if (k === "a") {
      throw new Error("boom");
    }
    return v;
  });
} catch (e: any) {
  console.log("caught " + e.message);
}

// The value-reified namespace static takes the same path.
const reified: any = JSON.stringify;
console.log(reified({ q: 3 }, function (k: string, v: any) {
  return typeof v === "number" ? v + 1 : v;
}));
