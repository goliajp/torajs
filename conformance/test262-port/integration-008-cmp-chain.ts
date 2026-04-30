// Integration: complex boolean expressions combining cmp + logical.
function inRange(x: number, lo: number, hi: number): boolean {
  return x >= lo && x <= hi;
}

function isWeekend(day: number): boolean {
  // 0 = Sun, 6 = Sat
  return day === 0 || day === 6;
}

function check(): number {
  if (inRange(5, 1, 10) !== true) { throw "#1"; }
  if (inRange(0, 1, 10) !== false) { throw "#2"; }
  if (inRange(10, 1, 10) !== true) { throw "#3"; }
  if (inRange(11, 1, 10) !== false) { throw "#4"; }
  if (isWeekend(0) !== true) { throw "#5"; }
  if (isWeekend(3) !== false) { throw "#6"; }
  if (isWeekend(6) !== true) { throw "#7"; }
  return 0;
}
console.log(check());
