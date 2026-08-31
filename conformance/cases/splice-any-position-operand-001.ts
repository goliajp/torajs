// §23.1.3.31 step 3 / §23.1.3.42 step 3 — the position and
// deleteCount slots are ToIntegerOrInfinity, so they reach every
// value. Both lanes handed the raw lowered operand to a helper whose
// params are i64, so an `any` arrived as its box bits and the array
// was spliced at the wrong place with no diagnostic at all.

const one: any = 1;
const oneStr: any = "1";
const undef: any = undefined;

const a = [9, 8, 7];
a.splice(one, 1);
console.log(a);

const b = [9, 8, 7];
console.log(b.splice(one, one), b);

const c = [9, 8, 7];
console.log(c.splice(oneStr, one), c);

const d = [9, 8, 7];
console.log(d.splice(one, undef), d);

const e = ["x", "y", "z"];
e.splice(one, 1);
console.log(e);

console.log([9, 8, 7].toSpliced(one, 1));
console.log([9, 8, 7].toSpliced(oneStr, one));
console.log([9, 8, 7].toSpliced(one, undef));

// The typed spellings keep their answers — including the arity
// defaults, which are the lane's own business and not ToInteger's.
const f = [9, 8, 7];
console.log(f.splice(1, 1), f);
const g = [9, 8, 7];
console.log(g.splice(1), g);
const h = [9, 8, 7];
console.log(h.splice(), h);
const i = [9, 8, 7];
console.log(i.splice(1, undefined), i);
const j = [9, 8, 7];
console.log(j.splice(-1, 1), j);
const k = [9, 8, 7];
console.log(k.splice(1, 1, 5, 6), k);
const l = [9, 8, 7];
console.log(l.splice(1.7, 1), l);
console.log([9, 8, 7].toSpliced(1, 1));
console.log([9, 8, 7].toSpliced(1, undefined));
console.log([9, 8, 7].toSpliced(1, 1, 5));
