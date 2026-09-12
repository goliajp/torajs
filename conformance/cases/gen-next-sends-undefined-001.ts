// RFC 20260805-generator-value-channels, the caller → body half: a
// bare `next()` sends `undefined` (§27.5.1.1.1), and the value of a
// `yield` expression is what was sent — on the typed lane, the any
// lane, a typed or an `any` receiver, from a `for…of` (which sends
// nothing), in a class generator method and an async generator.
// Before: the resume channel was typed by the yield type and a bare
// `next()` sent that type's zero, so `const a = yield 1` read `0`
// (and `for…of` sent `null` for a while in between).
function* h(): number {
  const a = yield 1; console.log("typed-bare", a, typeof a);
  const b = yield 2; console.log("typed-5", b, typeof b);
  const c = yield 3; console.log("typed-undef", c, typeof c);
}
const ih = h();
ih.next(); ih.next(); ih.next(5); ih.next(undefined);

function* mk(): any {
  const a = yield 1; console.log("any-bare", a, typeof a);
  const b = yield 2; console.log("any-5", b, typeof b);
}
const t: any = mk(); t.next(); t.next(); t.next(5);
const u = mk(); u.next(); u.next(undefined); u.next(7);

function* s(): string { const a = yield "x"; console.log("str-bare", a, typeof a); }
const is = s(); is.next(); is.next();

function* fo(): number { const a = yield 1; console.log("forof-sent", a, typeof a); }
for (const v of fo()) console.log("forof-v", v);

class C { *m(): number { const a = yield 1; console.log("class-gen", a, typeof a); } }
const ic = new C().m(); ic.next(); ic.next();

// `yield (yield 1)`: the inner value is the outer's operand
let count = 0;
function* nest() { yield (yield 1); count++; }
const iter = nest();
console.log(JSON.stringify(iter.next()), JSON.stringify(iter.next()), JSON.stringify(iter.next()), count);

// the sent value as a computed key of a literal the generator holds
function* ck() { const k = yield 10; let o = { [k]: 1, a: 'a' }; return o; }
const ick = ck(); ick.next();
const ock = ick.next().value;
console.log("computed-key", ock.a, ock[undefined], ock["undefined"], JSON.stringify(ock));
function* ck2() { let o = { [yield 10]: 1, a: 'a' }; yield 20; return o; }
const ick2 = ck2();
console.log(ick2.next().value, ick2.next().value);
const ock2 = ick2.next().value;
console.log("computed-key-inline", ock2[undefined], ock2.a);

async function* ag(): number { const a = yield 1; console.log("async-gen", a, typeof a); }
(async () => { const it = ag(); await it.next(); await it.next(); })();

