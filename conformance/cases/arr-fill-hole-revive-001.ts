// RFC 20260721-array-proto-cluster 刀 13d follow-up — §23.1.3.7 fill
// writes are Sets: a filled elision hole becomes a live default data
// property again. Pre-fix the stale F_HOLE shadow survived the fill
// and every hole-aware consumer (`in` / [[Get]] through the exotic
// gate) treated the freshly written index as still absent.
const r = [, , , , 0].fill(8, 1, 3);
console.log(0 in r, 1 in r, 2 in r, 3 in r, r[1], r[2]);
// clean receiver keeps the plain fill path
const c = [1, 2, 3].fill(9, 1);
console.log(c, 0 in c, 1 in c);
