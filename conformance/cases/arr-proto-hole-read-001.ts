// RFC 20260721-array-proto-cluster 刀 5 G3 — a hole (elision /
// length-grow / past-the-end write gap) is not an own property:
// `in` answers false, and element reads continue to the
// Array.prototype digit keys (getters run against the original
// receiver).

// length-grow marks holes (no proto keys installed yet). NOTE: the
// `in` probes ride an `any` receiver — the typed-lane `1 in c` shape
// compile-folds to a bounds check without hole semantics (recorded
// follow-up, out of 刀 5 scope).
const c: any[] = [0];
c.length = 2;
const ca: any = c;
console.log("1 in c:", 1 in ca);
console.log("c[1]:", c[1]);

// past-the-end write gap is a hole too
const g: any[] = [0];
g[3] = 9;
const ga: any = g;
console.log("1 in g:", 1 in ga, "3 in g:", 3 in ga, "g[1]:", g[1]);

// scalar-lane grow + proto data value: toString reads through the hole
Array.prototype[1] = 1;
let x = [0];
x.length = 2;
console.log("toString:", x.toString());

// any-receiver hole read sees the proto value; `in` walks the chain
console.log("a[1]:", ca[1]);
console.log("1 in c:", 1 in ca);
console.log("c[1]:", c[1]);

// includes Gets through the hole (§23.1.3.16 has no HasProperty gate)
const e: any[] = [0, , 2];
console.log("includes:", e.includes(1));
console.log("indexOf:", e.indexOf(1));

// toLocaleString invokes the inherited element's hook (n === 2)
let n = 0;
let hooked = {
  toLocaleString: function () {
    n++;
    return "";
  },
};
Array.prototype[1] = hooked;
let y: any[] = [hooked];
y.length = 2;
y.toLocaleString();
console.log("hooks:", n);

// mid-scan proto getter install (test262 15.4.4.14-9-a-10 shape)
let arr: any[] = [0, , 2];
Object.defineProperty(arr, "0", {
  get: function () {
    Object.defineProperty(Array.prototype, "1", {
      get: function () {
        return 6.99;
      },
      configurable: true,
    });
    return 0;
  },
  configurable: true,
});
console.log("indexOf getter:", arr.indexOf(6.99));

// backwards scan (15.4.4.15-8-a-10 shape) — getter now installed
let arr2: any[] = [0, , 2];
console.log("lastIndexOf getter:", arr2.lastIndexOf(6.99));
