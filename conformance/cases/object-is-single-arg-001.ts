// §20.1.2.9 Object.is(value1, value2): a missing argument is
// `undefined`. The typed lowering stood aside for fewer than two
// arguments and the generic lane had no shape for it (test262
// Object/is/not-same-value-x-y-null and siblings, rotation 651).
console.log(Object.is(null), Object.is(undefined), Object.is());
console.log(Object.is(0), Object.is(NaN), Object.is("undefined"));
let u: any = undefined;
let n: any = null;
console.log(Object.is(u), Object.is(n), Object.is(u, undefined));
const o = { k: 1 };
console.log(Object.is(o), Object.is(o, o), Object.is(null, undefined));
