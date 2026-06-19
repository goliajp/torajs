// S201 — String.prototype.padStart / padEnd accept 0-arg per ES
// §22.1.3.{16,17} step 1: maxLength defaults to undefined →
// ToLength(undefined) = 0, then step 2 returns the receiver unchanged
// because 0 ≤ S.length. Pre-S201 tr rejected at check.rs arity gate
// ("expected 2 argument(s), got 0").
//
// The 1-arg form (fill defaults to " ") is V3-18 m1.h.45 — already
// covered, included here as a regression spot.

console.log('padStart-no-arg:', 'abc'.padStart());
console.log('padEnd-no-arg:', 'abc'.padEnd());

// empty string receiver — step 2 still triggers (0 ≤ 0).
console.log('padStart-empty:', ''.padStart());
console.log('padEnd-empty:', ''.padEnd());

// 1-arg regression (existing fill-defaults-to-space path).
console.log('padStart-1arg:', 'ab'.padStart(5));
console.log('padEnd-1arg:', 'ab'.padEnd(5));

// 2-arg sanity hold.
console.log('padStart-2arg:', 'ab'.padStart(5, '*'));
console.log('padEnd-2arg:', 'ab'.padEnd(5, '*'));

// side-effect ordering — receiver expression must still evaluate
// (parity with bun, which invokes ToObject on the binding before
// dispatching the method).
let calls = 0;
const make = (): string => { calls++; return 'xy'; };
console.log('side-effect-padStart:', make().padStart());
console.log('side-effect-padEnd:', make().padEnd());
console.log('side-effect-count:', calls);

// chained — the 0-arg form returns a fresh String value that supports
// further methods, mirroring the 1-arg / 2-arg forms.
console.log('chain-len:', 'abc'.padStart().length);
console.log('chain-includes:', 'hello'.padEnd().includes('ell'));
