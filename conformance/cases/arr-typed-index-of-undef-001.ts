// indexOf / lastIndexOf of undefined over a number[]: a stored undefined
// (the sentinel) matches, a hole (same word) does not — §23.1.3.17 step
// 9.a / .20 step 8.a HasProperty — whether the needle is a number read
// that may be undefined or an `any` known only at run time.
function mk(): number[] { let a = [1.5, 2.5]; a[4] = 3.5; return a; }
const zs: number[] = [1.5];
let u = [1.5, 2.5]; u[1] = zs[9];
console.log(u.indexOf(zs[9]), u.lastIndexOf(zs[9]), mk().indexOf(zs[9]), mk().lastIndexOf(zs[9]));
console.log(u.indexOf(undefined as any), u.lastIndexOf(undefined as any), mk().indexOf(undefined as any), mk().lastIndexOf(undefined as any));
const w = mk(); w[3] = zs[9];
console.log(w.indexOf(zs[9]), w.lastIndexOf(zs[9]), w.indexOf(zs[9], 4), w.lastIndexOf(zs[9], 2), w.indexOf(3.5), w.indexOf(NaN));
console.log([1, 2, 3].indexOf("2" as any), [1, 2, 3].indexOf(2 as any), [1.5, 2.5].lastIndexOf(2.5), ["a", "b"].indexOf("b"));
console.log(mk().indexOf(2.5), mk().lastIndexOf(3.5), u.includes(zs[9]), mk().includes(zs[9]));
