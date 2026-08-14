// An `as` suffix is a type ascription, not a different value, but
// both halves of the receiver promotion read the wrapper instead of
// what it wraps. The DECL side: `const K: any = function () { this…
// }` promoted while the suffix spelling declared the same thing and
// died on `__this`.
const K = function (p: number) {
  (this as any).x = p;
} as any;
K.prototype.m = function (): number {
  return (this as any).x * 2;
};
K.s = function (): number {
  return 3;
};
const k = new K(5);

// The USE side: `(K as any).s()` is the spelling TS asks for whenever
// the member is not on the declared type, and it used to hide the
// binding from the member-object use shape.
console.log(k.m(), (K as any).s(), k instanceof K);

// Same inside a function body, where the stored function also reads
// an outer local.
function outer(a: number): number {
  const C = function (this: any, p: number) {
    this.x = p;
  } as any;
  C.prototype.get = function (): number {
    return a + (this as any).x;
  };
  return (new C(3) as any).get();
}
console.log(outer(7));
