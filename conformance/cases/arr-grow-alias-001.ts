// RFC 20260706-arr-grow-alias-stability B1 — the array cell is fixed
// across grow (slots live behind a data pointer; grow swaps the
// buffer), so every alias observes growth. Pre-B1 each of these
// shapes was silent-wrong + UAF: grow realloc'd the whole block,
// freed the old cell unconditionally, and wrote the new pointer back
// into only the receiver's slot.

// shape 1 — cross-fn param grow (the everyday JS shape)
function g(a: number[]): void {
  a.push(7);
}
function f1(): void {
  const t: number[] = [1, 2];
  g(t);
  console.log(t.length, t[2]);
}
f1();

// shape 2 — fn-local typed alias grow
function f2(): void {
  const t: number[] = [1, 2];
  const s: number[] = t;
  s.push(3);
  console.log(t.length, s.length, t[2]);
}
f2();

// shape 3 — double any-alias grow (escape-demoted Arr<Any>)
const t3: number[] = [1, 2];
const u3: any = t3;
const v3: any = t3;
u3.push(9);
console.log(u3.length, v3.length, t3.length);

// shape 4 — single-binding repeated grow keeps working (regression)
const u4: any = [1, 2];
u4.push(3);
u4.push(4);
u4.push(5);
console.log(u4.length, u4[4] === undefined, u4[2]);

// shape 5 — grow past several doublings through an alias, then read
// through the original binding (buffer realloc chain, cell stable)
function f5(): void {
  const t: number[] = [0];
  const s: number[] = t;
  for (let i = 1; i < 40; i++) s.push(i);
  console.log(t.length, t[39], t[0]);
}
f5();
