// RFC 20260713-accessor-void-kind blade 2 — the `.length` / `.size`
// special-prop reads over a DynObj receiver must run an accessor
// entry's getter (pre-fix the raw pair was boxed as data: the getter
// never fired and a throwing getter read fell through as null).

// throwing length getter — a plain read propagates the exception
var obj: any = { 0: 11 };
Object.defineProperty(obj, "length", {
  get: function () {
    throw new Error("boom");
  },
  configurable: true,
});
try {
  obj.length;
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}

// benign length getter — fires per read, value flows
var lens: any = {};
let reads = 0;
Object.defineProperty(lens, "length", {
  get: function () {
    reads++;
    return 7;
  },
  configurable: true,
});
console.log("length =", lens.length);
console.log("reads =", reads);

// data length regression — a plain { length: 5 } still answers 5
var data: any = { length: 5 };
console.log("data.length =", data.length);

// size accessor on a plain object
var sz: any = {};
Object.defineProperty(sz, "size", {
  get: function () {
    return 42;
  },
  configurable: true,
});
console.log("size =", sz.size);

// data size + Map size regression
var dsz: any = { size: 3 };
console.log("dsz.size =", dsz.size);
var m: any = new Map();
m.set("k", 1);
console.log("map.size =", m.size);
