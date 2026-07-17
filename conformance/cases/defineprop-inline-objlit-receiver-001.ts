// Inline object-literal receiver promotes to the dynobj lane —
// Object.defineProperty({}, k, desc) installs a live entry on the
// returned O instead of silently no-opping on the struct face
// (test262 dstr poisoned-getter cluster shape: the getter must fire
// through a destructured-parameter read and its throw must be
// catchable).

// getter installed on the returned object
const p: any = Object.defineProperty({}, "g", {
  get: function () {
    return 7;
  },
});
console.log(p.g); // 7

// poisoned getter throws through a destructured parameter read
const poisoned: any = Object.defineProperty({}, "poisoned", {
  get: function () {
    throw new Error("boom");
  },
});
let f = function ({ poisoned }) {};
let caught = false;
try {
  f(poisoned);
} catch (e) {
  caught = true;
}
console.log(caught); // true

// non-empty literal receiver keeps its data fields
const q: any = Object.defineProperty({ a: 1 }, "b", {
  get: function () {
    return 2;
  },
});
console.log(q.a, q.b); // 1 2

// defineProperties on an inline literal receiver
const r: any = Object.defineProperties(
  {},
  {
    x: { value: 10, writable: true },
    y: {
      get: function () {
        return 11;
      },
    },
  }
);
console.log(r.x, r.y); // 10 11
console.log("done");
