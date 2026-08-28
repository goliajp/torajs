// A descriptor's `value` read out of an `any` slot carries its type
// in a NaN box, so the define has to ask the box what it holds
// rather than assuming a heap pointer. Every tag in the table, on
// both an ordinary object and an array receiver, plus the redefine
// that made the mis-tag observable.
var vNum = 1;
var vStr = "s";
var vBool = true;
var vNull = null;
var vUndef = undefined;
var vObj = { k: 9 };
var vArr = [1, 2];
var vFloat = 1.5;
var vNaN = NaN;

const shapes: any[] = [vNum, vStr, vBool, vNull, vUndef, vObj, vArr, vFloat, vNaN];

for (let i = 0; i < shapes.length; i++) {
  const o: any = {};
  Object.defineProperty(o, "p", { value: shapes[i], configurable: true });
  console.log(i, typeof o.p, String(o.p));
  // redefining a configurable property replaces the value, which is
  // where a bogus heap tag would try to release the old one
  Object.defineProperty(o, "p", { value: shapes[i] });
  console.log(i, typeof o.p, String(o.p));
}

// the reduced form of test262's
// DefineOwnProperty/nan-equivalence-define-own-property-reconfigure
var v = 1;
var a = {};
Object.defineProperty(a, "p", { value: v, configurable: true });
Object.defineProperty(a, "p", { value: v });
console.log("reconfigured", (a as any).p);

// array receiver takes the same packed pair
var idx = 2;
const arr: any[] = [0, 0, 0];
Object.defineProperty(arr, "1", { value: vStr, configurable: true });
Object.defineProperty(arr, "1", { value: idx });
console.log(arr.join(","), arr.length);
