// RFC 20260721-array-proto-cluster 刀 7 G8 — sort semantics:
// §23.1.3.30.2 steps 5-8 fire BEFORE a user comparator (undefined
// never reaches it and sorts last, G8a), and the default compare
// ToStrings Obj elements so their ToPrimitive hooks fire (G8b).

// G8a — user comparator over mixed any[]: undefined-last pre-probe
const a: any[] = [undefined, 2, 1, "X", -1, "a", true, NaN, Infinity];
let sawUndef = false;
a.sort(function (x: any, y: any) {
  if (x === undefined || y === undefined) {
    sawUndef = true;
  }
  const xS = String(x);
  const yS = String(y);
  if (xS < yS) return 1;
  if (xS > yS) return -1;
  return 0;
});
console.log("saw undef:", sawUndef);
console.log("sorted:", a.join("|"));
console.log("last:", a[8]);

// default sort: undefined still sorts last on the any lane
const d: any[] = [undefined, "b", "a"];
d.sort();
console.log("default:", d.join("|"), "last:", d[2]);

// G8b — typed struct elements ToString through their hook
let counter = 0;
let object = {
  toString: function () {
    counter = counter + 1;
    return "";
  },
};
let pair = [object, object];
pair.sort();
console.log("hook fired:", counter >= 2);
