// rotation 240 — two pow kernel faces: (1) the ES §6.1.6.1.3 cell
// where ES differs from C99 (abs(base)==1, exponent ±∞ → NaN; C99
// says 1 — the self-ported lattice had pinned the C99 answer, and
// the unit test never caught it because a host-linked test binary
// resolves `no_mangle pow` to libSystem's); (2) `x ** 0.5` rides
// sqrt for correct rounding (the exp/ln path printed …373095).
console.log(1 ** Infinity);
console.log((-1) ** Infinity);
console.log(1 ** -Infinity);
console.log((-1) ** -Infinity);
console.log(Math.pow(-1, Infinity));
console.log(Math.pow(1, -Infinity));
console.log(2 ** 0.5);
console.log(Math.pow(2, 0.5));
console.log(9 ** 0.5);
console.log((-2) ** 0.5);
console.log(2 ** Infinity);
console.log(0.5 ** Infinity);
console.log(2 ** -Infinity);
console.log(1 ** 100);
console.log(NaN ** 0);
