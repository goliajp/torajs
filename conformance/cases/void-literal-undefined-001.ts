// RFC 20260713-array-proto-residual blade 5 — `void <literal>` folds
// to the plain `undefined` ident at parse time (ES §13.5.2: evaluate
// then discard; literals have no effects). The former Sequence
// wrapper defeated every undefined-shape probe downstream: an
// any-lane array literal packed `void 0` as null.
var src = [1, null, void 0];
console.log(src[2], typeof src[2]);
console.log(src.flat()[2]);
var x: any = undefined;
console.log(x === void 0, void 0 === undefined, typeof void 0);
var y: any = 5;
console.log(y !== void 0);
console.log(void "s", void true, void null);
console.log([void 0].indexOf(undefined));
