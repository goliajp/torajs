// S203 — Math unary methods accept 0-arg per ES default-undefined.
// Spec refs: §21.3.2.* step 1.
// `ToNumber(undefined) = NaN`, and every unary Math method returns
// NaN unchanged for a NaN argument. The 0-arg form therefore
// short-circuits to NaN without dispatching the runtime helper.
//
// Pre-S203 the declared `vec![Type::Number]` signature rejected at
// the generic arity gate with "expected 1 argument(s), got 0".

console.log('abs:', Math.abs());
console.log('floor:', Math.floor());
console.log('ceil:', Math.ceil());
console.log('round:', Math.round());
console.log('sqrt:', Math.sqrt());
console.log('log:', Math.log());
console.log('exp:', Math.exp());
console.log('sign:', Math.sign());
console.log('trunc:', Math.trunc());
console.log('cbrt:', Math.cbrt());
console.log('sin:', Math.sin());
console.log('cos:', Math.cos());
console.log('tan:', Math.tan());
console.log('asin:', Math.asin());
console.log('acos:', Math.acos());
console.log('atan:', Math.atan());
console.log('log2:', Math.log2());
console.log('log10:', Math.log10());
console.log('sinh:', Math.sinh());
console.log('cosh:', Math.cosh());
console.log('tanh:', Math.tanh());
console.log('asinh:', Math.asinh());
console.log('acosh:', Math.acosh());
console.log('atanh:', Math.atanh());
console.log('expm1:', Math.expm1());
console.log('log1p:', Math.log1p());
console.log('clz32:', Math.clz32());
console.log('fround:', Math.fround());
console.log('f16round:', Math.f16round());

// 1-arg regression — make sure the helper Call path still works
// after the short-circuit was added.
console.log('abs(-3):', Math.abs(-3));
console.log('floor(3.7):', Math.floor(3.7));
console.log('ceil(3.2):', Math.ceil(3.2));
console.log('sqrt(16):', Math.sqrt(16));
console.log('sign(-5):', Math.sign(-5));
console.log('trunc(7.9):', Math.trunc(7.9));
console.log('cbrt(27):', Math.cbrt(27));

// Math.NaN identity — Math.<f>(NaN) === NaN. Use `Number.isNaN` to
// avoid the NaN !== NaN footgun.
console.log('isNaN-abs:', Number.isNaN(Math.abs()));
console.log('isNaN-floor:', Number.isNaN(Math.floor()));
console.log('isNaN-sqrt:', Number.isNaN(Math.sqrt()));

// Math.min / Math.max identity elements stay correct (variadic
// path, separate spec semantics — not affected by S203 but worth
// guarding so the new arm doesn't accidentally intercept them).
console.log('max-0arg:', Math.max());
console.log('min-0arg:', Math.min());
