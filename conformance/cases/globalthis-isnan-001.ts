// §19.2.2/§19.2.3 global isFinite/isNaN on the globalThis fill list —
// ToNumber-coercing cells, distinct from the Number.* predicates.
const g: any = globalThis;
const inan = g["isNaN"];
console.log(typeof inan, inan.name, inan.length);
console.log(inan(NaN), inan(0), inan("abc"), inan("42"), inan());
console.log(inan === Number.isNaN);
const ifin = g["isFinite"];
console.log(typeof ifin, ifin.name, ifin.length);
console.log(ifin(1), ifin(Infinity), ifin("3.5"), ifin("x"), ifin());
console.log(ifin === Number.isFinite);
// ToNumber coercion: "  7 " trims to 7; null coerces to 0
console.log(inan(null), ifin(null), inan("  7 "), ifin("  7 "));
const d1 = Object.getOwnPropertyDescriptor(globalThis, "isNaN");
console.log(typeof d1, d1.writable, d1.enumerable, d1.configurable, typeof d1.value);
const d2 = Object.getOwnPropertyDescriptor(globalThis, "isFinite");
console.log(typeof d2, d2.writable, d2.enumerable, d2.configurable, typeof d2.value);
