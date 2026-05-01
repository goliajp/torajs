// Adapted from test262: built-ins/Math/{sin,cos,tan,asin,acos,atan,atan2,
// log2,log10,cbrt}/* — ten Math statics, all libm-backed via thin
// inkwell wrappers. JS spec mostly defers to libm (well-known IEEE-754
// results); we test exact integer outcomes and a few characteristic
// boundary points where libm rounds cleanly enough to compare directly.
function check(): number {
  // Trig at zero — exact.
  if (Math.sin(0) !== 0) { throw "#1: sin(0)"; }
  if (Math.cos(0) !== 1) { throw "#2: cos(0)"; }
  if (Math.tan(0) !== 0) { throw "#3: tan(0)"; }
  if (Math.asin(0) !== 0) { throw "#4"; }
  if (Math.atan(0) !== 0) { throw "#5"; }

  // Trig at characteristic angles — round to 6 decimals and compare ints.
  let s90 = Math.round(Math.sin(Math.PI / 2) * 1e6);
  if (s90 !== 1000000) { throw "#6: sin(pi/2)"; }
  let cpi = Math.round(Math.cos(Math.PI) * 1e6);
  if (cpi !== -1000000) { throw "#7: cos(pi)"; }
  let t45 = Math.round(Math.tan(Math.PI / 4) * 1e6);
  if (t45 !== 1000000) { throw "#8: tan(pi/4)"; }
  let asin1 = Math.round(Math.asin(1) * 1e6);
  let pi_2 = Math.round((Math.PI / 2) * 1e6);
  if (asin1 !== pi_2) { throw "#9: asin(1)"; }
  let acos_n1 = Math.round(Math.acos(-1) * 1e6);
  let pi6 = Math.round(Math.PI * 1e6);
  if (acos_n1 !== pi6) { throw "#10: acos(-1)"; }
  let atan1 = Math.round(Math.atan(1) * 1e6);
  let pi_4 = Math.round((Math.PI / 4) * 1e6);
  if (atan1 !== pi_4) { throw "#11: atan(1)"; }

  // atan2 — quadrant-aware.
  let a11 = Math.round(Math.atan2(1, 1) * 1e6);
  if (a11 !== pi_4) { throw "#12: atan2(1,1)"; }
  let a10 = Math.round(Math.atan2(1, 0) * 1e6);
  if (a10 !== pi_2) { throw "#13: atan2(1,0)"; }
  let a0n1 = Math.round(Math.atan2(0, -1) * 1e6);
  if (a0n1 !== pi6) { throw "#14: atan2(0,-1)"; }

  // Logarithms — exact at powers.
  if (Math.log2(1) !== 0) { throw "#15"; }
  if (Math.log2(2) !== 1) { throw "#16"; }
  if (Math.log2(8) !== 3) { throw "#17"; }
  if (Math.log2(1024) !== 10) { throw "#18"; }
  if (Math.log10(1) !== 0) { throw "#19"; }
  if (Math.log10(10) !== 1) { throw "#20"; }
  if (Math.log10(100) !== 2) { throw "#21"; }
  if (Math.log10(1000) !== 3) { throw "#22"; }

  // Cube root.
  if (Math.cbrt(0) !== 0) { throw "#23"; }
  if (Math.cbrt(1) !== 1) { throw "#24"; }
  if (Math.cbrt(8) !== 2) { throw "#25"; }
  if (Math.cbrt(27) !== 3) { throw "#26"; }
  if (Math.cbrt(-8) !== -2) { throw "#27: negative"; }
  return 0;
}
console.log(check());
