// Rotation 185 — the overwritten reduce accumulator releases its
// stake (pass-through cb grew every unique seed's rc by one per
// iteration and never freed it). Behavior lanes: pass-through obj
// seed, fresh-acc chain, no-init seed, reduceRight, sum.
const xs: any[] = [1, 2, 3];
const seed = { tag: "s" };
const out = xs.reduce((acc: any, v: any) => acc, seed);
console.log(out.tag, seed.tag);
const chain = xs.reduce((acc: any, v: any) => ({ n: acc.n + v }), { n: 0 });
console.log(chain.n);
const ys = [10, 20, 30];
const sum = ys.reduce((a, b) => a + b);
console.log(sum);
const rsum = ys.reduceRight((a, b) => a + b, 1);
console.log(rsum);
const strs = ["a", "b", "c"];
const cat = strs.reduce((a, b) => a + b, "x");
console.log(cat);
