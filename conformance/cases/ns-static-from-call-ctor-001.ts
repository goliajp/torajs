// RFC 20260808-construct-channel B6 刀 2 — the ns-static receiver
// channel: Array.from's cell is recv-first, so `.call(C, items)`
// hands the constructor through argv[0] into the §23.1.2.1 step-4
// Construct split (array-like → Construct(C, «len»), iterable →
// Construct(C)); elements land through the define-semantics store
// and step 6 writes length. A non-constructor thisArg keeps
// ArrayCreate; Reflect.construct over a plain fn-expr rides the same
// B1 kernel (the pre-B1 reflect path only knew the class registry).
var thisVal: any;
var C: any = function () { thisVal = this; (this as any).stamped = true; };
const al: any = { length: 2, '0': 'x', '1': 'y' };
const r1: any = Array.from.call(C, al);
console.log(r1.stamped, r1.length, r1[0], r1[1], r1.constructor === C, thisVal === r1);
const r2: any = Array.from.call(C, al, function (v: any, k: any) { return v + k; });
console.log(r2.stamped, r2.length, r2[0], r2[1]);
const r3: any = Array.from.call(C, [7, 8]);
console.log(r3.stamped, r3.length, r3[0], r3[1]);
const r4: any = Array.from.call(null, al);
console.log(r4.length, r4[0], r4[1]);
const rc: any = Reflect.construct(C, []);
console.log(rc.stamped, thisVal === rc);
const f: any = Array.from;
const r5: any = f([1, 2, 3]);
console.log(r5.length, r5[2]);
