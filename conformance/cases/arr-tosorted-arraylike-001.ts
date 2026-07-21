// arraylike receiver toSorted — §23.1.3.34 generic semantics:
// ascending per-[[Get]] element reads all fire before the first
// comparefn call; comparator admit precedes even the length getter.
let getCalls: any = [];
let arrayLike: any = {
  length: 3,
  get 0() { getCalls.push(0); return 2; },
  get 1() { getCalls.push(1); return 1; },
  get 2() { getCalls.push(2); return 3; },
};

// comparefn throw after all element reads (read order observable)
try {
  (Array.prototype as any).toSorted.call(arrayLike, function () { throw new Error("boom"); });
} catch (e: any) {
  console.log("caught:", e.message);
}
console.log(JSON.stringify(getCalls));

// normal comparator sort — product is a plain dense array
getCalls = [];
const sorted = (Array.prototype as any).toSorted.call(arrayLike, (a: any, b: any) => a - b);
console.log(JSON.stringify(sorted), JSON.stringify(getCalls));

// default comparator (ToString order)
const def = (Array.prototype as any).toSorted.call(arrayLike);
console.log(JSON.stringify(def));

// non-callable comparefn throws BEFORE the length getter fires
let lenCalls = 0;
const lenObj = { get length() { lenCalls++; return 0; } };
try {
  (Array.prototype as any).toSorted.call(lenObj, 42);
} catch (e: any) {
  console.log("nc:", e instanceof TypeError, lenCalls);
}

// own-prop install shape re-enters the generic arm
const o2: any = { 0: "b", 1: "a", length: 2 };
o2.toSorted = (Array.prototype as any).toSorted;
console.log(JSON.stringify(o2.toSorted()));
