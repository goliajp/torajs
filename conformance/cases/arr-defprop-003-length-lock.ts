// RFC 20260712-arr-exotic-define chunk D — Array length lock:
// defineProperty(arr, "length", {writable: false}) rejects every
// later length mutation (assign / define / fresh-index append) while
// same-value redefines stay legal per spec 10.1.6.3.
let a = [1, 2, 3];
Object.defineProperty(a, "length", { writable: false });
let d: any = Object.getOwnPropertyDescriptor(a as any, "length");
console.log("desc:", d.value, d.writable, d.enumerable, d.configurable);
try {
  a.length = 1;
  console.log("assign ok", a.length);
} catch (e) {
  console.log("assign threw:", e instanceof TypeError);
}
try {
  Object.defineProperty(a, "length", { value: 5 });
  console.log("define ok");
} catch (e) {
  console.log("define threw:", e instanceof TypeError);
}
Object.defineProperty(a, "length", { value: 3 });
console.log("same-value ok:", a.length);
try {
  Object.defineProperty(a, "3", { value: 9 });
  console.log("append ok");
} catch (e) {
  console.log("append threw:", e instanceof TypeError);
}
try {
  Object.defineProperty(a, "length", { writable: true });
  console.log("unlock ok");
} catch (e) {
  console.log("unlock threw:", e instanceof TypeError);
}
let o: any = a;
let kl = "length";
try {
  o[kl] = 0;
  console.log("dyn assign ok");
} catch (e) {
  console.log("dyn assign threw:", e instanceof TypeError);
}
console.log("final:", a.length, a[0]);
// e/c upgrades reject on an unlocked array too
let b = [1];
try {
  Object.defineProperty(b, "length", { configurable: true });
  console.log("c-up ok");
} catch (e) {
  console.log("c-up threw:", e instanceof TypeError);
}
try {
  Object.defineProperty(b, "length", { enumerable: true });
  console.log("e-up ok");
} catch (e) {
  console.log("e-up threw:", e instanceof TypeError);
}
// value+writable:false composite applies the resize then locks
let c = [1, 2, 3, 4];
Object.defineProperty(c, "length", { value: 2, writable: false });
console.log("composite:", c.length, c[1]);
try {
  c.length = 4;
  console.log("relock ok");
} catch (e) {
  console.log("relock threw:", e instanceof TypeError);
}
