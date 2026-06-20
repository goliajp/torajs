// ES §27.2.4.{7,8} — Promise.{resolve,reject}(value, ...trailing)
// trailing-arg ignore. Spec reads only args[0]; trailing
// silent-drops. tora's check.rs prior reject "expects 0 or 1
// arg, got N" — S263 widens via per-method carve-out + SSA-emit
// 1-arg arm widens `args.len() == 1` → `>= 1`.

// resolve — value preserved; trailing dropped.
const r1 = Promise.resolve(1);
const r2 = Promise.resolve(2, "extra");
const r3 = Promise.resolve(3, 99, "ignored");

console.log(typeof r1);
console.log(typeof r2);
console.log(typeof r3);

// resolve + .then to verify the value made it through and
// trailing args didn't substitute it. tr's then chain is
// `(v: T) => T` (same in/out type, see T-15.g.3 sig).
Promise.resolve(42, "trail", 99).then((v: number): number => {
    console.log(v);
    return v;
});
Promise.resolve(100, 1, 2, 3).then((v: number): number => {
    console.log(v);
    return v;
});
