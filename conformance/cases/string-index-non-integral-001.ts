// A Number key that is not an integer names a property of the string,
// not a code unit (§7.1.19, §10.4.3.5): "abc"[1.5] reads "1.5", which
// no string owns.
const s: string = "abc";
console.log(s[1.5]);
console.log(s[1]);
console.log(s[-1]);
console.log(s[5]);
const k: number = Number("2.5");
console.log(s[k]);
console.log(s[k] === undefined);
const nan: number = Number("x");
console.log(s[nan]);
const t: string = s.slice(1);
console.log(t[0.5], t[1]);
