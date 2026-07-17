// §10.4.2.4 ArraySetLength steps 15-19 (15.2.3.6-4-116 shape) —
// shrinking past a non-configurable index deletes what it can,
// stops there, and throws TypeError. Pre-fix the resize deleted
// everything silently.

const arrObj: any = [0, 1];
Object.defineProperty(arrObj, "1", { value: 1, configurable: false });
try {
  Object.defineProperty(arrObj, "length", { value: 1 });
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}
console.log(arrObj.length); // 2 (stopped at the locked index + 1)
console.log(arrObj[1]); // 1 (survived)

// deletable tail above the locked index still shrinks
const a2: any = [0, 1, 2, 3];
Object.defineProperty(a2, "1", { value: 1, configurable: false });
try {
  Object.defineProperty(a2, "length", { value: 0 });
} catch (e) {
  console.log("caught2:", e instanceof TypeError);
}
console.log(a2.length); // 2 (3 and 2 deleted, stopped at 1)

// ordinary arrays shrink freely
const a3: any = [0, 1, 2];
Object.defineProperty(a3, "length", { value: 1 });
console.log(a3.length, a3[1]); // 1 undefined
console.log("done");
