// String family any-dispatch slice 4 — isWellFormed / toWellFormed (mids 129-130).
const s: any = "hello";
const w: any = "aπ𝄞z";
const sub: any = "xxhelloxx".slice(2, 7);
console.log(s.isWellFormed());
console.log(w.isWellFormed());
console.log(sub.isWellFormed());
console.log(s.toWellFormed());
console.log(w.toWellFormed());
console.log(sub.toWellFormed());
console.log(s.isWellFormed.name, s.isWellFormed.length);
console.log(s.toWellFormed.name, s.toWellFormed.length);
