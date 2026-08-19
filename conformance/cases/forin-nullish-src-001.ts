// §14.7.5.6 ForIn/OfHeadEvaluation — a nullish for-in source
// enumerates nothing (no ToObject TypeError); other primitive sources
// enumerate their coerced object's enumerable keys.
for (const k in undefined) console.log("never", k);
for (const k in null) console.log("never2", k);
let n = 0;
for (const k in 42) n++;
console.log("num", n);
for (const k in true) n++;
console.log("bool", n);
for (const k in "ab") console.log("str", k);
console.log("done");
