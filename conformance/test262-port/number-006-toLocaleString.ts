// Adapted from test262: built-ins/Number/prototype/toLocaleString/* —
// tr's no-arg toLocaleString collapses to toString since the subset
// has no Intl / locale-aware formatting. Tests stay inside the value
// range where bun's en-US default also emits the canonical decimal
// form (no thousands separator), so the three runtimes agree.
function check(): number {
  // Single-digit / small magnitudes — bun en-US default still emits
  // the bare decimal form, identical to tr's output.
  if ((0).toLocaleString() !== "0") { throw "#1: zero"; }
  if ((7).toLocaleString() !== "7") { throw "#2: single digit"; }
  if ((-7).toLocaleString() !== "-7") { throw "#3: neg"; }
  if ((42).toLocaleString() !== "42") { throw "#4: two-digit"; }
  if ((-99).toLocaleString() !== "-99") { throw "#5: neg two-digit"; }
  if ((100).toLocaleString() !== "100") { throw "#6: hundred"; }
  if ((-100).toLocaleString() !== "-100") { throw "#7: neg hundred"; }
  if ((999).toLocaleString() !== "999") { throw "#8: just below 1k"; }

  // Roundtrip via concat — works for any value that bun + tr agree on.
  let prefix = "value=";
  if (prefix + (99).toLocaleString() !== "value=99") { throw "#9: concat"; }

  // Identity vs Number(s) parse path.
  let n = 50;
  if (n.toLocaleString() !== "50") { throw "#10: ident recv"; }

  return 0;
}
console.log(check());
