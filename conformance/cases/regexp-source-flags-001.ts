// re.source / re.flags answer fresh owned Strs; the owned record
// lets every consumer release them (rotation 185 — discard churn
// leaked one Str per read pre-fix).
const re = /ab+c/gi;
console.log(re.source, re.flags);
const d = re.source;
const e = re.flags;
console.log(d, e, d === "ab+c", e === "gi");
console.log(re.source.length, re.flags.length);
const re2 = new RegExp("x\\d+", "mu");
console.log(re2.source, re2.flags);
