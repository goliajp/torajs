// F1 — generator state-machine ForOf arm (RFC 20260728-gen-forof-yieldstar).

// typed array source
function* g1(): number {
  const src: number[] = [1, 2, 3];
  for (const v of src) { yield v }
}
for (const x of g1()) console.log(x);

// implicit-any generator, inline literal source
function* g2() {
  for (const v of [10, 20]) { yield v }
}
for (const x of g2()) console.log(x);

// generator delegating over another generator (the yield* manual shape)
function* inner(): number { yield 100; yield 200 }
function* outer(): number {
  const src: any = inner();
  for (const v of src) { yield v }
}
for (const x of outer()) console.log(x);

// break / continue inside the rewritten loop
function* g3(): number {
  for (const v of [1, 2, 3, 4, 5]) {
    if (v == 2) continue;
    if (v == 4) break;
    yield v;
  }
  yield 99;
}
for (const x of g3()) console.log(x);

// string source rides the @@iterator string leg
function* g4() {
  for (const c of "ab") { yield c }
}
for (const x of g4()) console.log(x);

// two for-of loops in one generator (distinct lifted fields)
function* g6() {
  for (const a of [1]) { yield a }
  for (const b of [2]) { yield b }
}
for (const x of g6()) console.log(x);
