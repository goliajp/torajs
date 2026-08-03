// Assignment-pattern destructuring defaults holding hoisted
// generator expressions (rotation 291): the binding lane recorded
// dstr_default_names (r245 V4 knife 1) but the assignment lane
// (`[g = function*(){}] = []` / `({ g = function*(){} } = {})`)
// never did — the hoisted `__genexpr_N` Ident had no wrap-axis key
// and the slot panicked at box_to_any. Recording also carries
// §8.4.5 NamedEvaluation onto the hoisted factory.

let a: any, b: any;
[a = function* () {}, b = function* named() {}] = [];
console.log(typeof a, typeof b);
console.log(a.name);
console.log(b.name);

let c: any;
({ c = function* () {} } = {});
console.log(typeof c, c.name);

// defaults that are NOT fn values keep working
let d: any, e: any;
[d = 7, e = "s"] = [];
console.log(d, e);

// for-await-of head, the t262 template family's exact shape
let xGen: any, gen: any;
async function fa() {
  for await ([xGen = function* x() {}, gen = function* () {}] of [[]]) {
    console.log(typeof xGen, xGen.name);
    console.log(typeof gen, gen.name);
  }
}
fa();
