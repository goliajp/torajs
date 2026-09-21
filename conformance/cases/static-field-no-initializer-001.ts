// §7.3.32 DefineField step 6 — "Else, let initValue be undefined". A
// static field with no initializer IS defined, and its value is
// `undefined`. tr answered that with a parse error, which refuses a
// program tsc accepts — strictPropertyInitialization checks instance
// properties only, and the read side agrees: `A.v` types `number`
// there, unsoundly and on purpose (microsoft/TypeScript#27899).
//
// The slot widens to `T | undefined`, which is what `let x: T;` with
// no reachable write already gets one desugar later — the static
// position was refused before it could reach the same rule.
//
// The alternative was to keep the declared `T` and let the slot carry
// this width's undefined sentinel, which a `number` slot has. That
// keeps `C.v.toFixed()` compiling, and it was tried: the sentinel's
// recognition does not survive a function-call boundary yet, so
// `static g(): number { return C.v }` answered NaN where the spec
// says undefined. Widening turns that same program into a refusal —
// a return-type mismatch — and a refusal is the honest half of the
// trade. Nothing this declaration now admits answers a wrong value.
class A {
  static n: number;
  static s: string;
  static f: () => number;
  static a: number[];
}
console.log("number  ", String(A.n), typeof A.n, A.n === undefined, "n" in A);
console.log("string  ", String(A.s), typeof A.s, A.s === undefined);
console.log("fn      ", typeof A.f, A.f === undefined);
console.log("array   ", typeof A.a, A.a === undefined);
// Registered, not pinned here: `String(x)` where `x` is an
// object-typed slot holding undefined throws rather than answering
// "undefined". That is older than this declaration —
// `const o: { a: number } | undefined = undefined; String(o)` throws
// the same way — so it is not this knife's to fix or to hide.

// The markers ride the same road: `?` already meant `T | undefined`,
// and `!` is a promise the checker takes at its word — neither
// changes what the slot holds before the first write.
class B {
  static q?: number;
  static d!: number;
}
console.log("optional", String(B.q), typeof B.q);
console.log("definite", String(B.d), typeof B.d);

// A write narrows the binding back to the declared type, so the reads
// after it are on the typed lane again.
class C {
  static v: number;
}
C.v = 3;
console.log("assigned", C.v + 1, C.v.toFixed(1));

// A method body reads the same slot.
class D {
  static m: number;
  static probe(): string {
    return typeof D.m;
  }
}
console.log("in method", D.probe());

// A private static field is the same declaration.
class E {
  static #p: number;
  static get() {
    return E.#p;
  }
}
console.log("private ", String(E.get()));

// §25.5.2.4 step 8.b — an undefined property is omitted.
console.log("json    ", JSON.stringify({ n: A.n, s: A.s, k: 1 }));

// The spellings that already worked, one branch unchanged.
class F {
  static bare;
  static withInit: number = 5;
  static inferred = "x";
}
console.log("bare    ", String(F.bare), "init", F.withInit, "inferred", F.inferred);
