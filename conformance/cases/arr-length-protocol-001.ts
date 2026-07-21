// RFC 20260721-array-proto-cluster 刀 6 G9 — length-write protocol:
// splice honors the length lock even in the no-op shape (9a), a
// length assignment runs the spec's two conversions (9c), and
// Object.create(arr) inherits the parent's inherent length (9d).

// 9a — locked length rejects splice before any mutation
let a = [0, 1, 2];
Object.defineProperty(a, "length", { writable: false });
let threw = false;
try {
  a.splice(1, 2, 4);
} catch (e) {
  threw = true;
}
console.log("splice threw:", threw, "arr:", a.join(","));
threw = false;
try {
  a.splice(0, 0);
} catch (e) {
  threw = true;
}
console.log("noop splice threw:", threw);
const aa: any = a;
threw = false;
try {
  aa.splice(0, 1);
} catch (e) {
  threw = true;
}
console.log("any splice threw:", threw, "len:", aa.length);

// 9c — `length = obj` runs valueOf twice (ToUint32 + ToNumber)
let count = 0;
let e1 = [];
(e1 as any).length = {
  valueOf: function () {
    count = count + 1;
    return 1;
  },
};
console.log("valueOf count:", count, "len:", e1.length);
let count2 = 0;
let e2 = [9, 9];
threw = false;
try {
  (e2 as any).length = {
    valueOf: function () {
      count2 = count2 + 1;
      return 2.5;
    },
  };
} catch (e) {
  threw = true;
}
console.log("fractional threw:", threw, "count:", count2, "len:", e2.length);

// 9d — Object.create(arr) answers the parent's inherent length
let b = [];
(b as any).p = 1;
let xx = Object.create(b);
console.log("x.length:", xx.length, "x.p:", xx.p);
let c = [7, 8, 9];
let yy = Object.create(c);
console.log("y.length:", yy.length);
