// ES §20.2.3.2 SetFunctionName — a static method's .name is the
// property key, not the desugared `__sm_<C>__<M>` mangled form (which
// leaked through both the fn-addr registry rows and the typed member
// fold). Covers the direct fold, the any-lane registry read, fn-print,
// and the injected Error.isError static.
class K {
  static sf(a: number): number {
    return a;
  }
}
console.log(K.sf.name, K.sf.length);
console.log(K.sf);
const v: any = K.sf;
console.log(v.name);
console.log(Error.isError.name, Error.isError.length);
