// F2 — generic yield* delegation
function* inner(): number { yield 1; yield 2 }
function* outer(): number {
  const g: any = inner();
  yield* g;
  yield 3;
}
for (const x of outer()) console.log(x);

// string operand
function* s() { yield* "ab" }
for (const x of s()) console.log(x);

// array through an ident (not a literal — misses the S2.28 lane)
function* a() {
  const xs: number[] = [10, 20];
  yield* xs;
}
for (const x of a()) console.log(x);

// known-gen direct call keeps the typed lane
function* k(): number { yield* inner(); yield 9 }
for (const x of k()) console.log(x);
