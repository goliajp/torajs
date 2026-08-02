// filter's keep test folds through ToBoolean (ES §23.1.3.7) — a
// non-Bool callback return (numbers, strings, boxed values) keeps
// the element iff truthy, mirroring the predicate-family fold.
// (Inline arrows still take the seeded-boolean contextual ret ann —
// that checker face is a registered follow-up; named fns and
// fn-exprs reach the lowering's coerce today.)
function cbNum(v) { return v > 1 ? 1 : 0; }
console.log([1, 2, 3].filter(cbNum));
function cbStr(v) { return v > 1 ? "y" : ""; }
console.log([1, 2, 3].filter(cbStr));
const xs: any[] = [1, 0, "x", "", null, 7];
console.log(xs.filter(function (v) { return v; }));
