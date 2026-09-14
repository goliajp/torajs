// §23.2.2.1 step 8.b — `<TypedArray>.from(src, mapFn, thisArg)` calls
// mapFn with thisArg as its receiver. The lane rides the any-method
// call, whose kernel reads the receiver-first flag, so a promoted
// named mapFn only needed the receiver-first shell; it got the plain
// one and died on `this.k` of an undefined receiver (rotation 649).
var obj = { k: 10 };
function f(this: any, x: number): number { return x + (this as any).k; }
console.log(Uint8Array.from([1, 2], f, obj).join(","));
console.log(Float64Array.from([1.5], function (this: any, x: number): number { return x + (this as any).k; }, obj).join(","));
function gs(this: any, x: number): number { return this === undefined ? 1 : 0; }
console.log(Int16Array.from([7], gs).join(","));
console.log(Array.from([3], f, obj).join(","));
