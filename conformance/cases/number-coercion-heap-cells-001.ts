// The sibling list, one call face over. §7.1.4 step 8 runs
// OrdinaryToPrimitive with the NUMBER hint over the receiver: a Date
// answers its time value through `valueOf`, every other cell falls to
// `toString` and NaN. The kernel does the whole ladder and the `any`
// spelling has always reached it — `Number(d as any)` gave 0 while
// `Number(d)` was `Number(Date) coercion not yet supported`.
const m = new Map<number, number>([[1, 2]]);
const s = new Set<number>([1]);
const wm = new WeakMap<object, number>();
const ws = new WeakSet<object>();
const wr = new WeakRef({});
const d = new Date(0);
const d2 = new Date(1234);
const re = /a+/g;
const p = Promise.resolve(1);

console.log(Number(d), Number(d2), Number(m), Number(s), Number(re));
console.log(Number(wm), Number(ws), Number(wr), Number(p));
console.log(Number(m.keys()), Number([1, 2].keys()));

// the `any` spelling, unchanged
const a: any = d;
console.log(Number(a), Number(m as any), Number(re as any));

// the implicit ToNumber sites reach the same ladder
console.log(+d, d * 1, d - 0, d2 - d);
console.log(m * 1, re * 1);

// and the rows that were already there
console.log(Number("3.5"), Number(true), Number(null), Number(undefined), Number([]), Number([7]), Number(10n));
