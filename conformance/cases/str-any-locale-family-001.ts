// String family any-dispatch slice 3 — normalize / toLocaleLowerCase /
// toLocaleUpperCase / lastIndexOf / search / matchAll (mids 124-128 +
// the Str lastIndexOf arm on mid 82).
const s: any = "abcabc";
const acc: any = "école"; // combining acute
const sub: any = "xxAbCdxx".slice(2, 6); // Substr receiver

console.log(acc.normalize());
console.log(acc.normalize("NFC").length);
console.log(acc.normalize("NFD").length);
console.log(("é" as any).normalize("NFD").length);
try {
  s.normalize("XXX");
} catch (e) {
  console.log("normalize RangeError:", (e as Error).message);
}

console.log(sub.toLocaleLowerCase());
console.log(sub.toLocaleUpperCase());
console.log(("Straße" as any).toLocaleUpperCase());

console.log(s.lastIndexOf("bc"));
console.log(s.lastIndexOf("bc", 3));
console.log(s.lastIndexOf("zz"));
console.log(s.lastIndexOf(""));
console.log(sub.lastIndexOf("C"));

console.log(s.search(/ca/));
console.log(s.search(/zz/));
console.log(sub.search(/[A-Z]/));
// second occurrence of the same literal — regression lane for the
// LICM-cached RegExp argv ledger (the box takes its own reference).
console.log(s.search(/ca/));
for (let i = 0; i < 2; i++) {
  console.log(s.match(/ab/) === null ? "null" : "hit");
}

const ms = [...s.matchAll(/ab/g)];
console.log(ms.length, ms[0][0], ms[0].index, ms[1].index);

console.log(s.normalize.name, s.normalize.length);
console.log(s.toLocaleLowerCase.name, s.toLocaleLowerCase.length);
console.log(s.lastIndexOf.name, s.lastIndexOf.length);
console.log(s.search.name, s.search.length);
console.log(s.matchAll.name, s.matchAll.length);
