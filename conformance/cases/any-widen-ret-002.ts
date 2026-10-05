// Positions that take the declared `string` of a widened call keep
// compiling: the program lowers as if the call kept that type.
function echo(s: string) { return s; }
const v: any = "a,b";
for (const p of echo(v).split(",")) console.log(p);
const h: string = "abc,";
console.log(h.includes(echo(v), 0, 1), h.endsWith(echo(v), 3, 1));
const xs: number[] = [1, 2];
console.log(xs.join(echo(v), 0));
