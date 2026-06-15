// Array<Any>.reduce with a callback whose ret is Any (e.g.
// `(a, b) => a` returning the accumulator unchanged) gets
// acc_ty = Type::Any in ssa-lower, but the `initial` literal
// lowers to raw bits (I64 / F64 / Bool / Str). Pre-fix
// `Store(init_v, acc_slot)` wrote raw 100 into the 8-byte
// AnyValue slot — the next `Load any, acc_slot` decoded it as
// garbage NaN-box bits and inspect SIGSEGV'd. NaN-box pack via
// box_to_any matches the callback ret path that already returns
// a real AnyValue.

const g: any[] = [1, 2, 3];
// callback ret = Number → acc_ty = Number → init=0 path
console.log(g.reduce((a: any, b: any) => a + b, 0));
// callback ret = Any (`a` flows through) → acc_ty = Any
console.log(g.reduce((a: any, b: any) => a, 100));
// same shape, init is i64 0
console.log(g.reduce((a: any, b: any) => a, 0));
// callback ret = Any, return b each iter → last elem wins
console.log(g.reduce((a: any, b: any) => b, 0));
// str init exercises the rc-aware box_to_any arm
const h: any[] = ["a", "b", "c"];
console.log(h.reduce((acc: any, x: any) => acc + x, ""));
// bool init
const i: any[] = [1, 2, 3];
console.log(i.reduce((acc: any, x: any) => acc, true));
// f64 init
const j: any[] = [1, 2];
console.log(j.reduce((acc: any, x: any) => acc, 1.5));
// null init
const k: any[] = [1];
console.log(k.reduce((acc: any, x: any) => acc, null));
