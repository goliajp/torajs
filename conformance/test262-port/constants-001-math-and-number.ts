// Adapted from test262: built-ins/Math/{PI,E,LN2,LN10,LOG2E,LOG10E,SQRT2,
// SQRT1_2}/* + built-ins/Number/{MAX_SAFE_INTEGER,...}/* — IEEE-754
// constants on the Math and Number namespaces. tr emits these as
// ConstF64 / ConstI64 operands at the Member access site — zero
// runtime cost, fully constant-foldable by LLVM.
function check(): number {
  // Math constants — round to ints to compare exactly.
  let pi6 = Math.round(Math.PI * 1e6);
  if (pi6 !== 3141593) { throw "#1: PI"; }
  let e6 = Math.round(Math.E * 1e6);
  if (e6 !== 2718282) { throw "#2: E"; }
  let ln2_6 = Math.round(Math.LN2 * 1e6);
  if (ln2_6 !== 693147) { throw "#3: LN2"; }
  let ln10_6 = Math.round(Math.LN10 * 1e6);
  if (ln10_6 !== 2302585) { throw "#4: LN10"; }
  let log2e_6 = Math.round(Math.LOG2E * 1e6);
  if (log2e_6 !== 1442695) { throw "#5: LOG2E"; }
  let log10e_6 = Math.round(Math.LOG10E * 1e6);
  if (log10e_6 !== 434294) { throw "#6: LOG10E"; }
  let sqrt2_6 = Math.round(Math.SQRT2 * 1e6);
  if (sqrt2_6 !== 1414214) { throw "#7: SQRT2"; }
  let sqrt1_2_6 = Math.round(Math.SQRT1_2 * 1e6);
  if (sqrt1_2_6 !== 707107) { throw "#8: SQRT1_2"; }

  // LN2 * LOG2E ≈ 1 — sanity identity.
  let id6 = Math.round(Math.LN2 * Math.LOG2E * 1e6);
  if (id6 !== 1000000) { throw "#9: LN2 * LOG2E identity"; }

  // SQRT2 * SQRT1_2 ≈ 1.
  let id2_6 = Math.round(Math.SQRT2 * Math.SQRT1_2 * 1e6);
  if (id2_6 !== 1000000) { throw "#10: SQRT2 * SQRT1_2 identity"; }

  // Number safety bounds.
  if (Number.MAX_SAFE_INTEGER !== 9007199254740991) { throw "#11"; }
  if (Number.MIN_SAFE_INTEGER !== -9007199254740991) { throw "#12"; }

  // Number.NaN / Infinity via Number.isNaN / isFinite.
  if (Number.isNaN(Number.NaN) !== true) { throw "#13"; }
  if (Number.isFinite(Number.POSITIVE_INFINITY) !== false) { throw "#14"; }
  if (Number.isFinite(Number.NEGATIVE_INFINITY) !== false) { throw "#15"; }
  if (Number.isFinite(0) !== true) { throw "#16"; }

  // EPSILON > 0.
  if (Number.EPSILON <= 0) { throw "#17"; }

  // MAX_VALUE > 1, MIN_VALUE > 0.
  if (Number.MAX_VALUE <= 1) { throw "#18"; }
  if (Number.MIN_VALUE <= 0) { throw "#19"; }
  if (Number.MIN_VALUE >= 1) { throw "#20"; }
  return 0;
}
console.log(check());
