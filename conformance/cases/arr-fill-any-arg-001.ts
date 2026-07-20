// Any fill value into typed arrays — checker admit paired with the
// shared coerce_push_value unbox at the store boundary (push twin,
// rotation 158 pairing discipline). Number / String elems × full /
// ranged fill; the Str lane's owned coerce settles after the loop.
const a: any = 9;
const out: number[] = [1, 2, 3];
out.fill(a);
console.log(out[0], out[1], out[2]);

const s: any = "x";
const strs: string[] = ["p", "q"];
strs.fill(s);
console.log(strs[0], strs[1]);

const r: any = 5;
const ranged: number[] = [1, 2, 3, 4];
ranged.fill(r, 1, 3);
console.log(ranged[0], ranged[1], ranged[2], ranged[3]);

const f: any = 2.5;
const fs: number[] = [1.5, 3.5];
fs.fill(f);
console.log(fs[0], fs[1]);

const arith: any = 4;
const mix: number[] = [0, 0];
mix.fill(arith * 2 + 1);
console.log(mix[0], mix[1]);
