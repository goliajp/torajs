// rotation 284 — a predicate callback's return folds through
// ToBoolean (ES §23.1.3.{8-11,30}): number and string rets are
// legal (TS stdlib spells predicates `=> unknown`). The lowering
// coerces the ret and releases an owned heap one after the
// truthiness read.
var arr = [1, 2, 3];
function cbNum(v) { return v > 1 ? 1 : 0; }
function cbStr(v) { return v > 1 ? "y" : ""; }
console.log(arr.every(cbNum));
console.log(arr.some(cbNum));
console.log(arr.find(cbStr));
console.log(arr.findIndex(cbStr));
console.log(arr.findLast(cbNum));
console.log(arr.filter(cbNum));
