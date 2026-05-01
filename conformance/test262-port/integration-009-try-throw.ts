// Integration: try / catch with thrown values flowing through nested
// fn calls + re-throwing. (`return` inside try-body alongside throw +
// finally hits a known v0 ssa-lower bug — covered separately.)
function check_pos(n: number): number {
  if (n < 0) { throw "neg"; }
  return n;
}

function safe_pos(n: number, fallback: number): number {
  try {
    return check_pos(n);
  } catch (e) {
    return fallback;
  }
}

function check(): number {
  if (safe_pos(10, -99) !== 10) { throw "#1"; }
  if (safe_pos(0, -99) !== 0) { throw "#2"; }
  if (safe_pos(-5, 42) !== 42) { throw "#3: caught"; }
  if (safe_pos(-100, 7) !== 7) { throw "#4"; }
  if (safe_pos(123, -1) !== 123) { throw "#5"; }

  // Re-throwing supported via bare `throw e;`.
  let depth: number = 0;
  try {
    try {
      depth = depth + 1;
      throw "outer-via-inner";
    } catch (e) {
      depth = depth + 10;
      throw e;
    }
  } catch (e2) {
    depth = depth + 100;
  }
  if (depth !== 111) { throw "#6"; }

  // Nested try; inner catches, outer doesn't see.
  let outer_caught: boolean = false;
  let inner_caught: boolean = false;
  try {
    try {
      throw "inner-only";
    } catch (e) {
      inner_caught = true;
    }
  } catch (e) {
    outer_caught = true;
  }
  if (inner_caught !== true) { throw "#7"; }
  if (outer_caught !== false) { throw "#8: outer should not catch"; }
  return 0;
}
console.log(check());
