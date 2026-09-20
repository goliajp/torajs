// The ITERATOR OBJECT itself re-parented: next / return come off its
// user chain, not off the class table its tag still points at. The
// typed spelling drives the same lanes (for-of, spread, destructuring,
// early break) as the any spelling.
class It {
  i = 0;
  next() { return this.i < 2 ? { done: false, value: 100 + this.i++ } : { done: true, value: undefined }; }
  return() { console.log("class return"); return { done: true, value: undefined }; }
  [Symbol.iterator]() { return this; }
}
function mkProto() {
  let n = 0;
  return {
    next() { return n < 3 ? { done: false, value: 200 + n++ } : { done: true, value: undefined }; },
    return() { console.log("user return"); return { done: true, value: undefined }; },
    [Symbol.iterator]() { return this; },
  };
}
const a: It = new It();
Object.setPrototypeOf(a, mkProto());
for (const v of a) console.log("for-of", v);
const b: It = new It();
Object.setPrototypeOf(b, mkProto());
console.log("spread", [...b].join(","));
// early break runs the USER chain's return(), not the class's
const c: It = new It();
Object.setPrototypeOf(c, mkProto());
for (const v of c) { console.log("break", v); break; }
const d: It = new It();
Object.setPrototypeOf(d, mkProto());
const [x, y] = d;
console.log("dstr", x, y);
// a bare user chain puts the class's next() off-chain entirely
const e: It = new It();
Object.setPrototypeOf(e, {});
console.log("off-chain", typeof (e as any).next);
try { (e as any).next(); } catch (err: any) { console.log("threw", err.constructor.name); }
