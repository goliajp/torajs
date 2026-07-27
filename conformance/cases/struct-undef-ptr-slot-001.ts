// RFC 20260710 C2b completion — a struct field slot that only ever
// saw nullish literals stays `Ptr`; it now takes the generic
// undefined cell for a written `undefined` (write side:
// str_undef_sentinel_for via spells_undef_with_generic_cell), and
// the compile-time any-member IC normalizes the cell back to
// `undefined` on read (read side: heap_slot_tag_value), mirroring
// the runtime probe. Before this the write collapsed undefined to
// NULL and the any-array lane answered `null`.

// 1) the recorded residual — undefined field read through the
//    hetero any-array lane
const a = [{ r: 2 }, { r: undefined }];
for (const o of a) {
  console.log(o.r);
}

// 2) direct faces on a pure-undefined field: strict-eq both ways,
//    typeof, nullish-coalescing, JSON (undefined fields are omitted)
const u = { r: undefined };
console.log(u.r === undefined, u.r === null, typeof u.r, u.r ?? 'fb');
console.log(JSON.stringify(u));

// 3) the null counterpart keeps its identity (NULL stays null)
const n = { r: null };
console.log(n.r, n.r === null, n.r === undefined, JSON.stringify(n));

// 4) any[] direct index read
const arr: any[] = [{ k: undefined }];
console.log(arr[0].k);
