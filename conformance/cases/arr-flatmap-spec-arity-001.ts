// §23.1.3.13 — flatMap's callback takes the full spec arity
// (elem, index, srcArray); shorter callbacks ride the prefix rule and
// the lowering appends the trailing slots per the declared arity.
const xs = [3, 1, 2];
const fm = xs.flatMap((v, i, a) => [v, i, a.length]);
console.log(fm.length, fm[1], fm[2]);
const fm2 = xs.flatMap((v, i) => [v * i]);
console.log(fm2.length, fm2[2]);
// fully annotated signature normalizes through the srcArray view
// promotion, same as the trio family
const fm3 = xs.flatMap((v: number, i: number, a: number[]) => [v, v]);
console.log(fm3.length, fm3[5]);
