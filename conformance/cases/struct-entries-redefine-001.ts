// §20.1.2.5 Object.entries walks EnumerableOwnProperties. On a
// statically typed receiver the member list is unfolded at compile
// time, and `Object.defineProperty(o, "a", {enumerable: false})` moves
// that set at run time — so the unfold stands behind the same
// redefined-member gate JSON.stringify already used.

const o = { a: 1, b: 2 };
console.log(Object.entries(o).map((e) => e[0] + "=" + e[1]).join(" "));
Object.defineProperty(o as any, "a", { enumerable: false });
console.log(Object.entries(o).map((e) => e[0] + "=" + e[1]).join(" "));
// The surfaces that already consulted the sidecar still agree.
console.log(Object.keys(o).join(","), JSON.stringify(o));
console.log(Object.getOwnPropertyNames(o).join(","), "a" in o, o.a);

// An accessor member answers its getter, and hides the same way.
const g = {
  get p() {
    return 10;
  },
  q: 20,
};
console.log(Object.entries(g).map((e) => e[0] + "=" + e[1]).join(" "));
Object.defineProperty(g as any, "p", { enumerable: false });
console.log(Object.entries(g).map((e) => e[0] + "=" + e[1]).join(" "));
console.log(g.p);

// An instance nobody redefined takes the unfold unchanged.
const untouched = { m: 1, n: 2 };
console.log(Object.entries(untouched).map((e) => e[0] + "=" + e[1]).join(" "));

// The gate is per instance, not per class.
const one = { u: 1, v: 2 };
const two = { u: 3, v: 4 };
Object.defineProperty(one as any, "u", { enumerable: false });
console.log(Object.entries(one).map((e) => e[0]).join(","));
console.log(Object.entries(two).map((e) => e[0]).join(","));
