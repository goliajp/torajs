// typed receiver + any separator must run the runtime three-way
// dispatch (was: raw AnyValue bits into the (Str, Str) kernel =
// SIGSEGV). assertions use join/index reads — the stringify walk of
// a typed Arr<Any> is a recorded separate face.
const x: any = "l";
const r1 = "hello".split(x);
console.log(r1.length, r1.join("|"), r1[0]);
const y: any = undefined;
const r2 = "hello".split(y);
console.log(r2.length, r2[0]);
const z: any = /l/;
const r3 = "hello".split(z);
console.log(r3.length, r3.join("|"), r3[2]);
const r4 = "hello".split(undefined as any);
console.log(r4.length, r4[0]);
const cm: any = ",";
const r5 = "a,b,c".split(cm, 2);
console.log(r5.length, r5.join("+"));
