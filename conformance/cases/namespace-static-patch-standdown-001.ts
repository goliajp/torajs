// The typed routes claim `Object.keys(o)` / `Math.max(a, b)` /
// `JSON.stringify(v)` off the callee's SYNTAX and never read the
// namespace object, so a write to one of its members was invisible —
// while the same patch reached `(Object as any).keys(o)` and an
// aliased `const O: any = Object; O.keys(o)`. A module that writes a
// namespace static stands those routes down for that member.

const o = { a: 1, b: 2 };
console.log("before", JSON.stringify(Object.keys(o)), Math.max(1, 2));

(Object as any).keys = function () {
  return ["patched"];
};
(Array as any).isArray = function () {
  return "nope";
};
(Math as any).max = function () {
  return -1;
};
(JSON as any).stringify = function () {
  return "PATCHED";
};

console.log("keys", (Object.keys(o) as any).join(","));
console.log("isArray", Array.isArray([1]) as any);
console.log("max", Math.max(1, 2) as any);
console.log("json", JSON.stringify({ b: 2 }) as any);

// the alias and the `as any` spelling answered the patch all along,
// and still do — one object, one answer
const O: any = Object;
console.log("alias", (O.keys(o) as any).join(","));
console.log("cast", ((Object as any).keys(o) as any).join(","));

// a member nobody wrote keeps its typed route
console.log("untouched", JSON.stringify(Object.getOwnPropertyNames({ z: 1 })));
