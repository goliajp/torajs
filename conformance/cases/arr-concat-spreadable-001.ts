// §23.1.3.1 concat: @@isConcatSpreadable decides spread vs append (§7.3.22),
// an absent index keeps a hole, a throwing getter propagates.
const obj: any = { length: 3, 0: "a", 2: "c" };
obj[Symbol.isConcatSpreadable] = true;
const r = [1].concat(obj);
console.log(r.length, JSON.stringify(r), 2 in r, 3 in r);
const arr: any = [1, 2];
arr[Symbol.isConcatSpreadable] = false;
const r2 = [0].concat(arr);
console.log(r2.length, r2[1] === arr);
const h = [1, , 3];
const r3 = [0].concat(h);
console.log(r3.length, 2 in r3, 3 in r3);
const r4: any[] = ([] as any[]).concat({ length: 2, 0: 5 });
console.log(r4.length, typeof r4[0]);
function f() { return 1; }
(f as any)[Symbol.isConcatSpreadable] = true;
(f as any)[0] = "z";
console.log(JSON.stringify([9].concat(f as any)));
const bad: any = { get [Symbol.isConcatSpreadable]() { throw new Error("boom"); } };
try { [].concat(bad); } catch (e: any) { console.log("caught", e.message); }
const typed: number[] = [4, 5];
console.log([1, 2].concat(typed, [6]).join());
const hr = [1, , 3];
const r5 = hr.concat([4]);
console.log(r5.length, 1 in r5, JSON.stringify(r5));
const sp: any = { length: 2, 0: "p", 1: "q", [Symbol.isConcatSpreadable]: true };
console.log(JSON.stringify(Array.prototype.concat.call(sp, 1)));
const rn = [].concat(null);
console.log(rn.length, rn[0]);
[].concat(undefined as any);
