// an absent number reads as undefined, and ToNumber(undefined) is NaN:
// every slot the value reaches must keep it NaN, never truncate it to 0
function t1(x?: number) { return x * 2 }
console.log(t1(), t1(3));
function a2(x?: number): number { return (x as any) + 1 }
console.log(a2(), a2(4));
function a3(x?: number) { return x + 1 }
console.log(a3(), a3(4));
function un(x: number | undefined) { const y = x * 2; return y }
console.log(un(undefined), un(2));
function un2(x: number | undefined): number { return x - 1 }
console.log(un2(undefined), un2(2));
function un3(x: number | undefined) { return 2 * x }
console.log(un3(undefined), un3(2));
function nn(x: number | null | undefined) { return x * 2 }
console.log(nn(undefined), nn(null), nn(4));
function mul(x: number | null) { return x * 2 + 1 }
console.log(mul(null), mul(3));
function lt(x?: number) { return x < 1 }
function ltn(x: number | null) { return x < 1 }
console.log(lt(), ltn(null), lt(0));
function mx(x?: number) { const m = Math.max(x, 1); return m }
console.log(mx(), mx(5));
function tpl(x?: number) { return `${x}` }
function tpln(x: number | null) { return `${x}` }
console.log(tpl(), tpln(null), tpl(7));
function co(x?: number) { return (x ?? 5) * 2 }
console.log(co(), co(0));
function of(o: { a?: number }) { return o.a * 2 }
console.log(of({}), of({ a: 3 }));
class C { a?: number; constructor() {} }
function cf(c: C) { return c.a * 2 }
console.log(cf(new C()));
interface I { v?: number }
function fi(o: I) { return o.v - 1 }
console.log(fi({}), fi({ v: 4 }));
function ae(a: (number | undefined)[]) { return a[1] * 2 }
console.log(ae([1, undefined]), ae([1, 2, undefined]));
function ru(b: boolean): number | undefined { return b ? 3 : undefined }
function useRu(b: boolean) { const r = ru(b); return r * 2 }
console.log(useRu(false), useRu(true));
function ul() { let x: number; return x * 2 }
console.log(ul());
function acc(xs: number[], k?: number) { let s = 0; for (const v of xs) s += v * k; return s }
console.log(acc([1, 2]), acc([1, 2], 3));
function sub(x?: number) { return 10 - x }
console.log(sub(), sub(4));
function sumOpt(a: number, b?: number) { return a + b }
console.log(sumOpt(1), sumOpt(1, 2));
