// Object.defineProperty on a primitive-wrapper receiver — pre-fix the
// wrapper cell was walked as a dynobj header (silent corruption,
// SIGSEGV on first define). StringWrapper inherent slots (length /
// char index) validate per §10.4.3.2 against the fixed §22.1.4
// attributes; every other key rides the lazy expando dynobj.
function mks(v: any): any {
  return new String(v);
}
function mkn(v: any): any {
  return new Number(v);
}
const w = mks("ab");
Object.defineProperty(w, "x", {
  value: 1,
  enumerable: true,
  writable: true,
  configurable: true,
});
console.log(w.x, Object.keys(w));
try {
  Object.defineProperty(w, "0", { value: "z" });
  console.log("no-throw");
} catch (e: any) {
  console.log("caught");
}
try {
  Object.defineProperty(w, "0", {
    value: "a",
    writable: false,
    enumerable: true,
    configurable: false,
  });
  console.log("compat-ok");
} catch (e: any) {
  console.log("compat-caught");
}
try {
  Object.defineProperty(w, "length", { value: 5 });
  console.log("len-no-throw");
} catch (e: any) {
  console.log("len-caught");
}
try {
  Object.defineProperty(w, "length", {
    value: 2,
    writable: false,
    enumerable: false,
    configurable: false,
  });
  console.log("len-compat-ok");
} catch (e: any) {
  console.log("len-compat-caught");
}
try {
  Object.defineProperty(w, "0", {
    get: () => "g",
  });
  console.log("acc-no-throw");
} catch (e: any) {
  console.log("acc-caught");
}
console.log(w[0], w.length);
Object.defineProperty(w, "5", {
  value: "q",
  enumerable: true,
  writable: true,
  configurable: true,
});
// keys order for a numeric expando key diverges (§10.1.11.1 merges
// integer indices across stores ascending; tr appends expando keys
// in insertion order) — recorded L3b, value face asserted only.
console.log(w[5]);
const n = mkn(3);
Object.defineProperty(n, "y", {
  value: 9,
  enumerable: true,
  writable: true,
  configurable: true,
});
console.log(n.y, Object.keys(n));
