// §20.1.2.22 Object.values over a statically typed receiver. The
// unfold keeps the array's element type — the general arm cannot be
// the any-lane walk, which answers Arr<Any> — so it emits the same
// unfold and asks per member whether a sidecar has hidden it.

const a = { x: 1, y: 2, z: 3 };
console.log(Object.values(a).join(","));
Object.defineProperty(a as any, "y", { enumerable: false });
console.log(Object.values(a).join(","), Object.entries(a).map((e) => e[0]).join(","));
// The array is still a number array, not a boxed one.
console.log(Object.values(a).reduce((s, v) => s + v, 0), Object.values(a).length);
// Hiding is not deleting.
console.log(a.y, "y" in a, Object.getOwnPropertyNames(a).join(","));
// And it goes back.
Object.defineProperty(a as any, "y", { enumerable: true });
console.log(Object.values(a).join(","));

// Same for a string-valued member, whose slot is a pointer.
const s = { p: "one", q: "two" };
Object.defineProperty(s as any, "p", { enumerable: false });
console.log(Object.values(s).join(","), s.p, Object.values(s).length);

// An accessor member contributes its getter's answer, and hides too.
const g = {
  get m() {
    return 5;
  },
  n: 6,
};
console.log(Object.values(g).join(","));
Object.defineProperty(g as any, "m", { enumerable: false });
console.log(Object.values(g).join(","), g.m);

// Hiding every member leaves an empty array, not a hole-filled one.
const e = { only: 7 };
Object.defineProperty(e as any, "only", { enumerable: false });
console.log(Object.values(e).length, JSON.stringify(Object.values(e)));

// An instance nobody redefined takes the plain unfold.
const u = { c: 1, d: 2 };
console.log(Object.values(u).join(","), Object.values(u).length);
