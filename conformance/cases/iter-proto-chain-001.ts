// sec 27.1.4 — iterator helpers resolve through a
// { __proto__: Iterator.prototype } literal's chain.
let n = 0;
const it = {
  __proto__: Iterator.prototype,
  next() { n += 1; return n <= 3 ? { done: false, value: n } : { done: true, value: undefined }; },
};
console.log((it as any).map((x: number) => x * 10).toArray().join(","));
// take/drop chain over a fresh source
function src() {
  let k = 0;
  return {
    __proto__: Iterator.prototype,
    next() { k += 1; return k <= 5 ? { done: false, value: k } : { done: true, value: undefined }; },
  };
}
console.log((src() as any).drop(1).take(2).toArray().join(","));
console.log((src() as any).filter((x: number) => x % 2 === 1).toArray().join(","));
// numeric validation still rejects loudly (sec 27.1.4.3 step 4)
try { (src() as any).drop(-1); } catch (e) { console.log("caught", (e as any)?.constructor?.name); }
console.log("survived");
