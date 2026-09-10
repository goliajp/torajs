// A function expression reading `this` handed to a method of a
// binding whose value came from a DYNAMIC construction (`new TA(3)`
// with `TA` a parameter — the value is `any` by construction, so the
// call rides the runtime any-method dispatch, which seeds the
// callback's receiver): §23.2.3.24 / §23.1.3.24 reduce and §23.2.3.29
// sort call the callback with `this` undefined; forEach passes its
// thisArg when given (test262 TypedArray/prototype/{reduce,sort}/
// callbackfn-this, comparefn-calls).
"use strict";
function probe(TA: any, label: string) {
  var sample = new TA(3);
  var seen: any[] = [];
  sample.reduce(function () { seen.push(this === undefined); return 0; }, 0);
  var sortThis = true;
  sample.sort(function (a: any, b: any) { sortThis = sortThis && this === undefined; return a - b; });
  seen.push(sortThis);
  var obj = { tag: label };
  sample.forEach(function () { seen.push(this === obj); }, obj);
  sample.forEach(function () { seen.push(this === undefined); });
  console.log(label, seen.join(","));
}
probe(Uint8Array, "u8");
probe(Float64Array, "f64");
var plain: any = [3, 1, 2];
var seen2: any[] = [];
plain.reduce(function (acc: any, v: any) { seen2.push(this === undefined); return acc + v; }, 0);
plain.reduceRight(function (acc: any) { seen2.push(this === undefined); return acc; }, 0);
var sortThis2 = true;
plain.sort(function (a: any, b: any) { sortThis2 = sortThis2 && this === undefined; return a - b; });
seen2.push(sortThis2);
console.log("plain", seen2.join(","), plain.join(""));
