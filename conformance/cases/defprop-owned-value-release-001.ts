// defineProperty with a runtime-minted value: the desc value temp
// must release on success AND on every §10.1.6.3 rejection leg
// (churn probes w4-w7 verified flat)
const o: any = {};
Object.defineProperty(o, "k", {
  value: "minted-" + 42,
  enumerable: true,
  writable: true,
  configurable: true,
});
console.log(o.k);

// rejection: fresh key on a non-extensible object
const p: any = {};
Object.preventExtensions(p);
let caught = "";
try {
  Object.defineProperty(p, "n", { value: "x" + 1 });
} catch (e: any) {
  caught = e.name;
}
console.log(caught);
console.log(Object.keys(p).length);

// rejection: readonly value change keeps the original value
const r: any = {};
Object.defineProperty(r, "k", { value: "init-" + 7, writable: false, configurable: false });
let caught2 = "";
try {
  Object.defineProperty(r, "k", { value: "changed-" + 8 });
} catch (e: any) {
  caught2 = e.name;
}
console.log(caught2);
console.log(r.k);

// rejection: non-extensible wrapper expando
const w: any = new String("ab");
Object.preventExtensions(w);
let caught3 = "";
try {
  Object.defineProperty(w, "n", { value: "y" + 2 });
} catch (e: any) {
  caught3 = e.name;
}
console.log(caught3);

// repeated success redefine lands the latest value
const q: any = {};
for (let i = 0; i < 3; i++) {
  Object.defineProperty(q, "k", { value: "v-" + i, writable: true, configurable: true });
}
console.log(q.k);
