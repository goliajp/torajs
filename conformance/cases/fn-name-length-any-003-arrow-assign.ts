// assign-position NamedEvaluation: the reassigned arrow takes the
// target binding's name (ES 13.15.2)
let f = (a: number, b: number) => a + b;
let f0: any = f;
f = (a: number, b: number) => a * b;
let fv: any = f;
console.log(f0.name, fv.name, fv.length, f(3, 4));
// let-position regression guard (chunk 720)
let k = (x: number) => x - 1;
let kv: any = k;
console.log(kv.name, kv.length, k(10));
// chained assign: only the innermost names the arrow
let p = (x: number) => x;
let q = (x: number) => x;
p = q = (x: number) => x + 7;
let pv: any = p;
let qv: any = q;
console.log(pv.name, qv.name, p(1), q(2));
