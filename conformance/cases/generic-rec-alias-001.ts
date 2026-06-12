// recursive generic alias instantiation — compile-time stack overflow
// regression (check + lower in-flight guards) and the nominal fract
// width face (rfcs/20260612-generic-recursive-alias)
type Rec<T> = { v: T; next: Rec<T> | null };
let r: Rec<number> = { v: 0.25, next: null };
console.log(r.v);
let r2: Rec<number> = { v: 1.5, next: r };
console.log(r2.next!.v);
console.log(r2.v);
type Wrap<T> = { inner: Rec<T> };
let w: Wrap<number> = { inner: { v: 2.5, next: null } };
console.log(w.inner.v);
type A<T> = { v: T; b: B<T> | null };
type B<T> = { v: T; a: A<T> | null };
let a: A<number> = { v: 7, b: { v: 8, a: null } };
console.log(a.v);
console.log(a.b!.v);
