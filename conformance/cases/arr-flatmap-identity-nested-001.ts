// §23.1.3.13 flatMap with an identity callback flattens one level;
// the integer inner blocks must survive the width class the callback's
// annotated ret seeds (pre-fix the product read the i64 slots as f64
// denormals — 1.5e-323 for 3).
const grid = [[1, 2], [3]];
const flatInts = grid.flatMap((x) => x);
console.log(flatInts.length, flatInts[0], flatInts[2]);
const gridF = [[1.5, 2.5], [3.5]];
const flatFracs = gridF.flatMap((x) => x);
console.log(flatFracs.length, flatFracs[0], flatFracs[2]);
const gridS = [["a", "b"], ["c"]];
const flatStrs = gridS.flatMap((x) => x);
console.log(flatStrs.length, flatStrs[0], flatStrs[2]);
// mixed-width source: fractional evidence anywhere in the class
// widens every inner block consistently
const gridM = [[1, 2], [3.5]];
const flatMixed = gridM.flatMap((x) => x);
console.log(flatMixed.length, flatMixed[1], flatMixed[2]);
