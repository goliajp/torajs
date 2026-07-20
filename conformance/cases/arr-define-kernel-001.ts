// RFC 20260721-object-descriptor-cluster 刀 5 (R-G) — arr define
// kernel: SameValue on readonly redefine, the data→accessor
// configurable pre-gate, and the typed-elem-store escape.

// bug a — same-content Str redefine on a readonly index is SameValue
var a: any = [];
Object.defineProperty(a, "0", { value: "abcd" });
try {
  Object.defineProperties(a, { "0": { value: "abcd" } });
  console.log("same-value redefine ok:", a[0]);
} catch (e) {
  console.log("same-value redefine THREW");
}
try {
  Object.defineProperty(a, "0", { value: "other" });
  console.log("changed-value redefine NO-THROW:", a[0]);
} catch (e) {
  console.log("changed-value redefine threw:", e instanceof TypeError, a[0]);
}
// mixed-width number pair is the same Number value
var n: any = [];
Object.defineProperty(n, "0", { value: 5 });
try {
  Object.defineProperty(n, "0", { value: 5.0 });
  console.log("mixed-width redefine ok:", n[0]);
} catch (e) {
  console.log("mixed-width redefine THREW");
}

// bug b — nonconfig data → accessor rejects BEFORE clearing the slot
var b: any = [];
Object.defineProperty(b, "1", { value: 3, configurable: false });
try {
  Object.defineProperties(b, { "1": { set: function () {} } });
  console.log("nonconfig->accessor NO-THROW");
} catch (e) {
  console.log("nonconfig->accessor threw:", e instanceof TypeError, "val=", b[1]);
}
// configurable data → accessor still lands
var b2: any = [];
Object.defineProperty(b2, "0", { value: 7, configurable: true, enumerable: true });
Object.defineProperty(b2, "0", { get: function () { return 42; } });
console.log("config->accessor:", b2[0]);

// bug c — typed element store escapes to the any lane
var c = [1, 2, 3];
Object.defineProperty(c, 1, { value: "abc" });
console.log("int-arr define str:", c[1], c[0], c[2]);
