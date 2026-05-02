// Anonymous arrow callbacks (no param annotations) on Array
// methods now work — the lifted-closure FnDecl's param + return
// type annotations are inferred from the call site's expected
// callback signature (looked up via the receiver array's type
// annotation and the method name).
//
// Previously each anonymous callback required explicit
// annotations: `arr.sort((a: number, b: number): number => a - b)`.
// Now `arr.sort((a, b) => a - b)` — far more idiomatic — works.

function check(): number {
  // sort: (T, T) => number
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
  xs.sort((a, b) => a - b);
  if (xs[0] !== 1) { throw "#1: min"; }
  if (xs[xs.length - 1] !== 9) { throw "#2: max"; }

  // map: (T) => T (we infer to elem-typed; richer cases need ann)
  let doubled: number[] = xs.map((v) => v * 2);
  if (doubled[0] !== 2) { throw "#3: doubled[0]"; }

  // filter: (T) => boolean
  let evens: number[] = xs.filter((v) => v % 2 === 0);
  if (evens.length !== 3) { throw "#4: evens count"; }   // [2, 4, 6]

  // forEach: (T) => void — observed via accumulator (Copy capture
  // by-ref; mutation propagates through closure-by-ref machinery).
  let sum: number = 0;
  xs.forEach((v) => { sum = sum + v; });
  if (sum !== 44) { throw "#5: sum"; }

  // reduce: (acc, cur) => acc — both T-typed in our MVP; accum-typed
  // reductions to a different type still need ann.
  let total: number = xs.reduce((a, c) => a + c, 0);
  if (total !== 44) { throw "#6: reduce"; }

  // some / every: (T) => boolean
  let has_nine: boolean = xs.some((v) => v === 9);
  if (!has_nine) { throw "#7: has 9"; }
  let all_pos: boolean = xs.every((v) => v > 0);
  if (!all_pos) { throw "#8: all positive"; }

  // findIndex: (T) => boolean
  let i_of_4: number = xs.findIndex((v) => v === 4);
  if (xs[i_of_4] !== 4) { throw "#9: findIndex"; }

  return 0;
}
console.log(check());
