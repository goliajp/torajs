// RFC 20260719-select-formation route 3 -- the cmp-sink pass moves a
// single-use ICmp down past the speculated arm instructions so the
// adjacency-gated NZCV fuse fires (cset; cmp #0; csel collapses to
// cmp; csel). Semantics must be identical with the pass on, off
// (TORAJS_CMP_SINK_OFF=1), and under tr run.
function pickF(n: number): number {
  let s = 0;
  let a = 3;
  let b = 2;
  for (let i = 0; i < n; i++) {
    s = i % 3 > 0 ? a + i : b - i;
    a = s + 1;
    b = s - 1;
  }
  return s;
}
function pickI(n: number): number {
  let s = 0;
  let a = 7;
  let b = 5;
  for (let i = 0; i < n; i++) {
    s = (i & 1) === 0 ? a + i : b - i;
    a = s + 2;
    b = s - 2;
  }
  return s;
}
// The counting shape stays with the CSINC fuse -- the sink must not
// displace its ADD (cmp; cinc beats add; cmp; csel).
function countOdd(n: number): number {
  let c = 0;
  for (let i = 0; i < n; i++) {
    if ((i & 1) === 1) {
      c = c + 1;
    }
  }
  return c;
}
console.log(pickF(1000), pickF(7), pickF(0));
console.log(pickI(1000), pickI(9), pickI(1));
console.log(countOdd(1000), countOdd(3));
