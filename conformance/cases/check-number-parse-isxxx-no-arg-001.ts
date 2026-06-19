// S202 — global parseInt/parseFloat/isNaN/isFinite and the Number.*
// counterparts accept 0-arg per ES default-undefined.
//
// Spec refs:
//   §19.2.{3,4,5} global isFinite / isNaN / parseInt / parseFloat
//   §21.1.2.{3,5,7,12,13} Number.is{Integer,NaN,Finite,SafeInteger} /
//                          Number.parseFloat / Number.parseInt
//
// All four parse* methods read `string` and default to undefined →
// ToString → "undefined" → parse fails → NaN.
// Global isNaN/isFinite apply ToNumber(undefined)=NaN, giving the
// fixed answers isNaN→true, isFinite→false.
// Strict Number.is* don't coerce, so the implicit-undefined arg fails
// the Number-shape gate → false.

// Global parse*.
console.log('parseInt-no-arg:', parseInt());
console.log('parseFloat-no-arg:', parseFloat());

// Global isNaN / isFinite — ToNumber(undefined) = NaN.
console.log('isNaN-no-arg:', isNaN());
console.log('isFinite-no-arg:', isFinite());

// Number.parse* — alias path, same NaN answer.
console.log('Number.parseInt-no-arg:', Number.parseInt());
console.log('Number.parseFloat-no-arg:', Number.parseFloat());

// Number.is* strict family — non-Number → false.
console.log('Number.isNaN-no-arg:', Number.isNaN());
console.log('Number.isFinite-no-arg:', Number.isFinite());
console.log('Number.isInteger-no-arg:', Number.isInteger());
console.log('Number.isSafeInteger-no-arg:', Number.isSafeInteger());

// 1-arg / 2-arg regression — ensure the loose arity-gate didn't
// break the existing happy paths.
console.log('parseInt-1arg:', parseInt('42'));
console.log('parseInt-2arg:', parseInt('ff', 16));
console.log('parseFloat-1arg:', parseFloat('3.14'));
console.log('isNaN-1arg:', isNaN('not a number'));
console.log('isFinite-1arg:', isFinite('3'));
console.log('Number.parseInt-1arg:', Number.parseInt('-7'));
console.log('Number.parseFloat-1arg:', Number.parseFloat('2.5e1'));
console.log('Number.isNaN-NaN:', Number.isNaN(NaN));
console.log('Number.isFinite-Inf:', Number.isFinite(1 / 0));
console.log('Number.isInteger-3:', Number.isInteger(3));
console.log('Number.isSafeInteger-2pow53:', Number.isSafeInteger(2 ** 53));
