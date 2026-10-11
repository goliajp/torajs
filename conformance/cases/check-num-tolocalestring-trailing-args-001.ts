// ES §21.1.3.4 — Number.prototype.toLocaleString(locales?, options?)
// trailing-arg ignore. Only en-US is formatted: a served locale with a
// primitive `options` (no option names to read) formats as usual, and
// arguments past the second are evaluated and dropped. S260 widens
// check.rs's fixed-arity sig (vec![Any, Any] = 2 optional) to accept
// trailing per ES.

const a: number = 1234.5;
const b: number = 1000000;
const c: number = 0;

// 0/1/2-arg base.
console.log(a.toLocaleString());
console.log(a.toLocaleString("en-US"));
console.log(b.toLocaleString());

// 3+ arg trailing — silent drop, identical to 1-arg result.
console.log(a.toLocaleString("en-US", "extra", 99));
console.log(b.toLocaleString("en-US", 1, 2, 3));
console.log(c.toLocaleString("en-US", "ignored", null));
