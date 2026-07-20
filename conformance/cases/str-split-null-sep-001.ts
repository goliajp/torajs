// typed lane `s.split(null)` — §22.1.3.23 step 15: separator null is
// not undefined, so it splits by ToString(null) = "null" (was SIGSEGV:
// ConstPtrNull fell into the (Str, Str) kernel ABI).
let s = "thisnullisnullanullstringnullobject";
let r = s.split(null);
console.log(r.length);
console.log(r[0]);
console.log(r.join("|"));
let t = "anullbnullc".split(null, 2);
console.log(t.length);
console.log(t.join("|"));
let z = "anullb".split(null, 0);
console.log(z.length);
let w = "no-sep-here".split(null);
console.log(w.length, w[0]);
