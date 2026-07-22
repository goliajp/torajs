// typed-tier Date.toJSON — an invalid date answers null
// (§21.4.4.37 steps 2-3), never toISOString RangeError; the
// JSON.stringify Date arm (any graph) serializes it bare.
const bad = new Date(NaN);
console.log(bad.toJSON());
console.log(bad.toJSON() === null);
const j = bad.toJSON();
console.log(j, typeof j);
const ok = new Date(0);
console.log(ok.toJSON());
console.log(typeof ok.toJSON());
const xs: any[] = [bad, ok];
console.log(JSON.stringify(xs));
