// Adapted from test262: built-ins/Number/prototype/{toFixed,toString}/* —
// fixed-point and default decimal serialization. tr's runtime uses
// snprintf("%.*f") for toFixed and %lld / %g for toString. Both
// dispatch on receiver SSA type at lower-time (i64 vs f64 picks the
// cheaper formatter).
//
// Subset note: rounding is libc-default (typically round-half-to-even
// on macOS/glibc), which matches JS engines for the common cases.
function check(): number {
  // toFixed — basic decimal formatting.
  if ((3.14).toFixed(2) !== "3.14") { throw "#1"; }
  if ((3.14).toFixed(1) !== "3.1") { throw "#2"; }
  if ((3.14).toFixed(0) !== "3") { throw "#3"; }
  if ((3.14).toFixed(4) !== "3.1400") { throw "#4: trailing zeros"; }

  // toFixed — integer receiver.
  if ((42).toFixed(2) !== "42.00") { throw "#5"; }
  if ((-7).toFixed(0) !== "-7") { throw "#6: negative"; }

  // toFixed — small / large.
  if ((0).toFixed(3) !== "0.000") { throw "#7"; }

  // toString — default formatting.
  if ((42).toString() !== "42") { throw "#8"; }
  if ((-100).toString() !== "-100") { throw "#9"; }
  if ((0).toString() !== "0") { throw "#10"; }
  if ((3.14).toString() !== "3.14") { throw "#11"; }

  // Chained — toString into includes.
  if ((12345).toString().includes("234") !== true) { throw "#12: chained"; }

  // Variable receivers — let inference picks i64 / f64 from the
  // initializer's literal kind (so `let x = 1.5` is f64-typed).
  let x = 1.5;
  if (x.toFixed(2) !== "1.50") { throw "#13: var receiver f64"; }
  let i = 100;
  if (i.toString() !== "100") { throw "#14: var receiver i64"; }
  return 0;
}
console.log(check());
