// RFC 20260712-arr-exotic-define chunk C — consumer surfaces honor
// per-index attribute shadow flags: dynamic-key writes route to
// element storage with a writable gate, propertyIsEnumerable /
// Object.keys / for-in read the enumerable bit.
let a = [];
Object.defineProperty(a, "0", { value: 42 });
let o: any = a;
let k = "0";
console.log("pIE:", o.propertyIsEnumerable(k));
let seen = "";
for (const x in o) {
  seen += x + ",";
}
console.log("forin:", seen);
try {
  o[k] = 99;
} catch (e) {
  console.log("ro threw:", e instanceof TypeError);
}
console.log("elem:", o[k], a[0]);
console.log("keys:", Object.keys(a).join("|"), "names:", Object.getOwnPropertyNames(a).join("|"));
// writable index routes the dynamic-key write into element storage
let b = [];
Object.defineProperty(b, "0", { value: 1, writable: true, enumerable: true, configurable: true });
let ob: any = b;
ob[k] = 7;
console.log("writable write:", ob[k], b[0]);
console.log("keys b:", Object.keys(b).join("|"), "names b:", Object.getOwnPropertyNames(b).join("|"));
// dynamic "length" key hits the resize path
let kl = "length";
ob[kl] = 0;
console.log("len write:", b.length);
// mixed enumerability across several indices
let c = [10, 20, 30];
Object.defineProperty(c, "1", { value: 21, enumerable: false, writable: true, configurable: true });
console.log("keys c:", Object.keys(c).join("|"), "elem:", c[1]);
let seenC = "";
let oc: any = c;
for (const x in oc) {
  seenC += x;
}
console.log("forin c:", seenC);
console.log("pIE c0:", oc.propertyIsEnumerable("0"), "pIE c1:", oc.propertyIsEnumerable("1"));
