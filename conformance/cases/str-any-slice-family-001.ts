// String slice-family any-dispatch backfill — substring / substr / at /
// charCodeAt on any receivers (mids 115-118): Substr views materialize
// through owned_src; at OOB answers undefined, charCodeAt OOB answers
// NaN per spec; the own-property + gOPD + name/length reflection face
// rides the RFC 20260712 machinery.
const s: any = "hello world";
console.log(s.substring(6));
console.log(s.substring(4, 1)); // swapped bounds
console.log(s.substring(-3, 5)); // negative clamps to 0
console.log(s.substr(6));
console.log(s.substr(-5, 3)); // negative start wraps
console.log(s.substr(2, 100)); // length clamps
console.log(s.at(0), s.at(-1), s.at(99));
console.log(s.charCodeAt(0), s.charCodeAt(10), s.charCodeAt(99));
console.log(s.charCodeAt());
// substr view receiver (split product element)
const parts: any = "alpha,beta".split(",");
console.log(parts[1].substring(1, 3), parts[1].substr(1, 2), parts[1].at(-1), parts[1].charCodeAt(2));
// wide chars
const w: any = "日本語abc";
console.log(w.substring(1, 4), w.substr(3, 2), w.at(1), w.charCodeAt(0));
// own-property + descriptor face
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "substr"));
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "substring"));
const d: any = Object.getOwnPropertyDescriptor(String.prototype, "at");
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log((s.charCodeAt as any).name, (s.charCodeAt as any).length);
console.log((s.substring as any).name, (s.substring as any).length);
