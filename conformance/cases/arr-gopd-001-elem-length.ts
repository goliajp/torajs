// RFC 20260712-arr-exotic-define chunk A — runtime gOPD TAG_ARR arm
// (length / canonical index / expando) + dynamic-string-key member
// read of Array length / elements through an `any` receiver.
let a = [10, "x", true];
let o: any = a;
let d0: any = Object.getOwnPropertyDescriptor(o, "0");
console.log(d0.value, d0.writable, d0.enumerable, d0.configurable);
let d1: any = Object.getOwnPropertyDescriptor(o, "1");
console.log(d1.value, d1.writable);
let dl: any = Object.getOwnPropertyDescriptor(o, "length");
console.log(dl.value, dl.writable, dl.enumerable, dl.configurable);
console.log(Object.getOwnPropertyDescriptor(o, "5"));
console.log(Object.getOwnPropertyDescriptor(o, "01"));
let arr2 = [1, 2];
let o2: any = arr2;
o2.tagx = "hello";
let de: any = Object.getOwnPropertyDescriptor(o2, "tagx");
console.log(de.value, de.writable, de.enumerable, de.configurable);
// dynamic string key reads (pre-fix: undefined for every Array recv)
let kLen = "length";
let kIdx = "2";
let kOob = "9";
console.log(o[kLen], o[kIdx], o[kOob]);
