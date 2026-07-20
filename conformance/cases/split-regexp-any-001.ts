// §22.1.3.23 step 2 — a RegExp separator through the any lane must
// hand off to @@split (was: ToString-coerced to the literal "/pat/"
// spelling, answering one unsplit token).
const a: any = "a1b2c3";
const r1 = a.split(/[0-9]/);
console.log(r1.length, r1.join("|"));
const s: any = new String("abc");
const r2 = s.split(new RegExp("[a-z]"));
console.log(r2.length, JSON.stringify(r2));
const c: any = "aXbXc";
const r3 = c.split(/(X)/);
console.log(r3.length, r3.join(","));
const r4 = a.split(/z/);
console.log(r4.length, r4[0]);
