// Adapted from test262: passing an object to a function — verifies the
// receiver sees the field values written by the caller.
type Box = { w: number, h: number };

function area(b: Box): number {
  return b.w * b.h;
}

function check(): number {
  let a: Box = { w: 3, h: 4 };
  let b: Box = { w: 5, h: 6 };
  if (area(a) !== 12) { throw "#1"; }
  if (area(b) !== 30) { throw "#2"; }
  return 0;
}
console.log(check());
