// §15.5.5 — `yield` stays legal in every function* body shape (fn
// decl / object method / class method / async generator / fn expr)
// while the r290 parse-time early error rejects it everywhere else
// (module code is strict, §16.1; negatives are sweep-verified since
// a parse reject has no bun-parity stdout).
function* g() { yield 1; const a = yield 2; yield* [3, 4]; return a; }
const it = g();
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next(9).value);
console.log(it.next().value);
console.log(it.next().value);
const obj = { *m() { yield "om"; } };
console.log(obj.m().next().value);
class C { *cm() { yield "cm"; } }
console.log(new C().cm().next().value);
async function* ag() { yield "ag"; }
ag().next().then((r: any) => console.log(r.value));
const fe = function* () { yield "fe"; };
console.log(fe().next().value);
