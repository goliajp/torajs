// L3b #4 — release through `any` for the former fallback-arm types:
// RegExp / Date / Symbol / Promise route to their rc-aware type drops;
// Closure routes through the env's synthesized drop_fn slot. Looped
// constructions so a double-free via pool/allocator reuse would trip.

// 1) RegExp released through any — compiled Program + src_bytes freed.
for (let i = 0; i < 3; i++) {
  const r: any = new RegExp("pat-" + i);
  console.log(r.source);
}

// 2) typed + any dual owners; any drops first, typed still usable.
const re = /alpha[0-9]+/;
{
  const ra: any = re;
  console.log(ra.source);
}
console.log(re.test("alpha42"));

// 3) Date released through any.
for (let i = 0; i < 3; i++) {
  const d: any = new Date(1700000000000 + i);
  console.log(d.getTime());
}

// 4) Symbol released through any — desc string freed with the cell.
for (let i = 0; i < 3; i++) {
  const s: any = Symbol("desc-heap-string-payload-" + i);
  console.log(typeof s);
}

// 5) Closure released through any — env drop_fn walks capture boxes.
function mk(i: number): () => number {
  const payload = "closure-capture-heap-string-payload-" + i;
  const f = (): number => {
    return payload.length;
  };
  return f;
}
for (let i = 0; i < 3; i++) {
  const f: any = mk(i);
  console.log(f());
}

// 6) closure typed + any dual owners; any drops first (dec gate must
//    not fire the unconditional walk while the typed side lives).
const keep = mk(7);
{
  const fa: any = keep;
  console.log(fa());
}
console.log(keep());
