// RFC 20260807-global-object — §19.2.4/§19.2.5 parseFloat/parseInt on
// the globalThis fill list, reusing the Number.* cells (§21.1.2.12/.13
// "the same built-in function object").
const g: any = globalThis;
const pi = g["parseInt"];
console.log(typeof pi, pi.name, pi.length);
console.log(pi("42", 10), pi("0x1f"), pi("  7abc"));
console.log(pi === Number.parseInt);
const pf = g["parseFloat"];
console.log(typeof pf, pf.name, pf.length);
console.log(pf("3.5"), pf("2.5e2xyz"));
console.log(pf === Number.parseFloat);
const d1 = Object.getOwnPropertyDescriptor(globalThis, "parseInt");
console.log(typeof d1, d1.writable, d1.enumerable, d1.configurable, typeof d1.value);
const d2 = Object.getOwnPropertyDescriptor(globalThis, "parseFloat");
console.log(typeof d2, d2.writable, d2.enumerable, d2.configurable, typeof d2.value);
