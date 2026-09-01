// rotation 553 — a builtin array HOF's callback receives Closure-repr
// elements, so its fn-typed element-position parameter carries the
// env-first shape (551-01). The element→param hand-off happens inside
// the lowered loop — no AST Call edge — so the param-tag pass's
// call-site rounds never saw it: `fs.map((f) => f())` was a silent
// zero-output death, and the same shape embedded in larger programs
// SIGBUS'd (blr into a closure cell).
const s = (n: number): string => "v" + n;
const fs: Array<() => string> = [];
for (let i = 0; i < 5; i++) {
  const t = s(i);
  fs.push(() => t + "!");
}

// The silent-death shape: map with a fn-typed elem param.
console.log(fs.map((f: () => string): string => f()).join(","));

// filter / find / some / every over the same elements.
const picked = fs.filter((f: () => string): boolean => f().length > 2);
console.log(picked.length);
const first = fs.find((f: () => string): boolean => f() === "v3!");
console.log(first ? first() : "none");
console.log(fs.some((f: () => string): boolean => f() === "v0!"));
console.log(fs.every((f: () => string): boolean => f().length === 3));

// forEach with an untyped-return callback.
let acc = "";
fs.forEach((f: () => string): void => {
  acc = acc + f();
});
console.log(acc);

// sort compares two elements — both positions carry the shape.
const sorted = fs.toSorted(
  (a: () => string, b: () => string): number => b().length - a().length
);
console.log(sorted.length);

// reduce's element sits at user-param 1.
const joined = fs.reduce(
  (r: string, f: () => string): string => r + f(),
  ""
);
console.log(joined);

// A named-fn callback seeds the same way.
function callIt(f: () => string): string {
  return f();
}
console.log(fs.map(callIt).join("|"));

// Index reads and for-of stay on their own (already working) lanes.
const f0 = fs[0];
console.log(f0());
for (const f of fs) {
  acc = f();
}
console.log(acc);
