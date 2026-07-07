// User-comparator sort never passes undefined to the callback —
// §23.1.3.30.2 SortCompare steps 5-8 run BEFORE the comparator, so
// undefined elements sort last unconditionally (even under a
// descending comparator). Covers the __torajs_arr_sort_cb fast path
// (mode bit 3 = Str-element undefined pre-probe).

const m = /a(b)?/.exec("a");
if (m !== null) {
  // 1) Ascending comparator — undefined sinks past "z".
  const xs = ["z", m[1], "a"];
  xs.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  console.log(xs.join(","));
  console.log(xs[2] === undefined);

  // 2) Descending comparator — undefined STILL last.
  const ys = ["b", m[1], "c"];
  ys.sort((a, b) => (a > b ? -1 : a < b ? 1 : 0));
  console.log(ys.join(","));
  console.log(ys[2] === undefined);

  // 3) The callback never observes undefined (call counts differ
  // between engines' sort algorithms; the flag is stable).
  let sawUndef = false;
  const zs = [m[1], "q", m[1], "p"];
  zs.sort((a, b) => {
    if (a === undefined || b === undefined) {
      sawUndef = true;
    }
    return a < b ? -1 : a > b ? 1 : 0;
  });
  console.log(zs.join(","));
  console.log(zs[2] === undefined, zs[3] === undefined);
  console.log(sawUndef);
}

// 4) Numeric comparator lane unchanged (no Str pre-probe emitted).
const ns = [3, 1, 2];
ns.sort((a, b) => a - b);
console.log(ns.join(","));
