// §22.1.3.19 / §22.1.3.23 / §22.2.3.1 — an `any` pattern, separator or
// flags value that is an array or an object runs ToString inside the
// runtime kernel; the program must keep the array / object dispatch
// arms that ToString re-enters.
const s = "ab0c12,3";
const a: any = [12];
console.log(s.replace(a, "Z"), s.replaceAll(a, "Y"));
const o: any = { toString() { return "c1"; } };
console.log(s.replace(o, "Z"));
const sep: any = [","];
console.log(s.split(sep));
const r: any = ["b0"];
const re = new RegExp(r);
console.log(re.source, s.match(re) !== null);
