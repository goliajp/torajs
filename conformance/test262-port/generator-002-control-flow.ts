// Phase J.2.b — `yield` inside `if` / `while` / `for` control flow,
// plus `break` / `continue` inside a yield-containing loop. The
// generator body lowers to a `while (true)` state machine where each
// yield is one resume point and control flow that crosses a yield
// becomes `state = N; continue;` gotos in the wrapping loop.
//
// All `let`s anywhere in the body (top-level, for-init, if-branch,
// loop-body) get lifted to fields on the iterator class so they
// survive yield boundaries.
//
// MVP scope: generator parameters are `number` only — passing an
// array into a generator hits a pre-J pre-existing class-field
// drop bug (shared reference between the call-site array and the
// iterator field's slot) so we work in pure-number space here.

function* count_up_to(n: number): number {
  let i = 0;
  while (i < n) {
    yield i;
    i = i + 1;
  }
}

function* range_for(n: number): number {
  for (let k = 0; k < n; k++) {
    yield k;
  }
}

// if / else, both branches yielding
function* signed(n: number): number {
  for (let j = 0; j < n; j++) {
    if (j % 2 === 0) {
      yield j;
    } else {
      yield -j;
    }
  }
}

// break inside a yield-containing loop
function* take_while_lt(n: number, limit: number): number {
  let i = 0;
  while (i < n) {
    if (i >= limit) { break; }
    yield i;
    i = i + 1;
  }
}

// continue inside a yield-containing loop (skip even numbers)
function* odds_below(n: number): number {
  for (let p = 0; p < n; p++) {
    if (p % 2 === 0) { continue; }
    yield p;
  }
}

// Naked if (no else) with a yield in the then-branch only
function* maybe_yield(n: number, gate: number): number {
  let r = 0;
  while (r < n) {
    if (r === gate) {
      yield 999;
    }
    yield r;
    r = r + 1;
  }
}

// Nested while inside a yield-while; both contribute yields
function* triangle(n: number): number {
  let row = 0;
  while (row < n) {
    let col = 0;
    while (col <= row) {
      yield col;
      col = col + 1;
    }
    row = row + 1;
  }
}

function check(): number {
  // count_up_to(3) → 0, 1, 2 then done
  let g1 = count_up_to(3);
  if (g1.next().value !== 0) { throw "#1: count_up_to[0]"; }
  if (g1.next().value !== 1) { throw "#2: count_up_to[1]"; }
  if (g1.next().value !== 2) { throw "#3: count_up_to[2]"; }
  if (g1.next().done !== true) { throw "#4: count_up_to end"; }
  if (g1.next().done !== true) { throw "#5: count_up_to re-end"; }

  // range_for(0) — empty range, immediately done
  let g0 = range_for(0);
  if (g0.next().done !== true) { throw "#6: empty range"; }

  // range_for(4) drained via the protocol loop
  let g2 = range_for(4);
  let total: number = 0;
  while (true) {
    let st = g2.next();
    if (st.done) { break; }
    total = total + st.value;
  }
  if (total !== 6) { throw "#7: range sum"; }

  // signed(4) → 0, -1, 2, -3
  let g3 = signed(4);
  if (g3.next().value !== 0) { throw "#8: signed[0]"; }
  if (g3.next().value !== -1) { throw "#9: signed[1]"; }
  if (g3.next().value !== 2) { throw "#10: signed[2]"; }
  if (g3.next().value !== -3) { throw "#11: signed[3]"; }
  if (g3.next().done !== true) { throw "#12: signed end"; }

  // take_while_lt(10, 3) → 0, 1, 2 then break
  let g4 = take_while_lt(10, 3);
  if (g4.next().value !== 0) { throw "#13: take[0]"; }
  if (g4.next().value !== 1) { throw "#14: take[1]"; }
  if (g4.next().value !== 2) { throw "#15: take[2]"; }
  if (g4.next().done !== true) { throw "#16: take break"; }

  // odds_below(8) → 1, 3, 5, 7
  let g5 = odds_below(8);
  if (g5.next().value !== 1) { throw "#17: odds[0]"; }
  if (g5.next().value !== 3) { throw "#18: odds[1]"; }
  if (g5.next().value !== 5) { throw "#19: odds[2]"; }
  if (g5.next().value !== 7) { throw "#20: odds[3]"; }
  if (g5.next().done !== true) { throw "#21: odds end"; }

  // maybe_yield(3, 1) → 0 (r=0, no gate), 999 (r=1, gate hit),
  //                    1 (r=1 yielded), 2 (r=2), done
  let g7 = maybe_yield(3, 1);
  if (g7.next().value !== 0) { throw "#22: maybe[0]"; }
  if (g7.next().value !== 999) { throw "#23: maybe gate"; }
  if (g7.next().value !== 1) { throw "#24: maybe[1]"; }
  if (g7.next().value !== 2) { throw "#25: maybe[2]"; }
  if (g7.next().done !== true) { throw "#26: maybe end"; }

  // triangle(3) → 0 | 0 1 | 0 1 2  (row 0, row 1, row 2)
  let g8 = triangle(3);
  if (g8.next().value !== 0) { throw "#27: tri row0"; }
  if (g8.next().value !== 0) { throw "#28: tri row1[0]"; }
  if (g8.next().value !== 1) { throw "#29: tri row1[1]"; }
  if (g8.next().value !== 0) { throw "#30: tri row2[0]"; }
  if (g8.next().value !== 1) { throw "#31: tri row2[1]"; }
  if (g8.next().value !== 2) { throw "#32: tri row2[2]"; }
  if (g8.next().done !== true) { throw "#33: tri end"; }

  return 0;
}
console.log(check());
