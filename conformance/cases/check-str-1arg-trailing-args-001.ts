// S240 — String.{at,charAt,charCodeAt,codePointAt,repeat,normalize}
// (useful, ...trailing) trailing-arg ignore per ES §22.1.3.{1,2,3,4,
// 17,13}: spec reserves slots past the single useful arg but tora's
// helpers are 1-arg only. Trailing operand type_of'd for side effects
// then dropped at lower-time (ssa_lower break early past i=0). Same
// shape as S238 localeCompare.

const s = "hello";
console.log(s.at(1, 99));
console.log(s.at(1, "junk" as any));
console.log(s.charAt(2, 99));
console.log(s.charAt(2, "junk" as any));
console.log(s.charCodeAt(0, 99));
console.log(s.charCodeAt(0, "junk" as any));
console.log(s.codePointAt(0, 99));
console.log(s.codePointAt(0, "junk" as any));

console.log("ab".repeat(3, 99));
console.log("ab".repeat(3, "junk" as any));
console.log("ab".repeat(undefined, 99));

console.log("é".normalize("NFC", 99));
console.log("é".normalize("NFC", "junk" as any));
console.log("é".normalize(undefined, 99));
