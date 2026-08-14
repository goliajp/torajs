// 383-04 (partial) — §B.2.2.15/16 trimLeft / trimRight are OWN
// properties of String.prototype in their own right: distinct keys
// over the same reified function cells. The enumeration face and
// the hasOwnProperty face must agree (they disagreed: names walked
// mids one-name-per-mid, ownership interned all four spellings).
const names = Object.getOwnPropertyNames(String.prototype);
console.log(names.includes("trimLeft"), names.includes("trimRight"));
console.log(String.prototype.hasOwnProperty("trimLeft"), String.prototype.hasOwnProperty("trimRight"));
console.log((String.prototype as any).trimLeft === (String.prototype as any).trimStart);
console.log((String.prototype as any).trimRight === (String.prototype as any).trimEnd);
const s = "  ab  ";
console.log(s.trimLeft() + "|" + s.trimRight() + "|");
const a: any = s;
console.log(a.trimLeft() + "|" + a.trimRight() + "|");
