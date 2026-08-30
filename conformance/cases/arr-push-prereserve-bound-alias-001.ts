// `for (let i = 0; i < src.length; i++) { dst.push(_) }` reserves
// capacity from `src.length` read once above the loop. That is a real
// invariant only while `src` and `dst` are two arrays; if they are one,
// each push moves the bound and the reservation runs short. Three ways
// two names become one array, all of which the proof must refuse — a
// refusal shows up as the right answer, since the fast path would end
// the loop early at 15.
function two_params(a: number[], b: number[]): number {
  for (let i: number = 0; i < (a.length >> 1); i++) { b.push(i); }
  return b.length;
}
let x: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
console.log(two_params(x, x));

let shared: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
function get_shared(): number[] { return shared; }
function reassigned(): number {
  let dst: number[] = [];
  dst = get_shared();
  for (let i: number = 0; i < (shared.length >> 1); i++) { dst.push(i); }
  return dst.length;
}
console.log(reassigned());

function aliased(): number {
  let src: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  let dst: number[] = src;
  for (let i: number = 0; i < (src.length >> 1); i++) { dst.push(i); }
  return dst.length;
}
console.log(aliased());

// And the shape all of this exists to allow: two arrays that really are
// two, filled by each other's length.
function copy(): number {
  let src: number[] = [];
  for (let i: number = 0; i < 100; i++) { src.push(i * 2); }
  let dst: number[] = [];
  for (let i: number = 0; i < src.length; i++) { dst.push(src[i]); }
  return dst.length + dst[99];
}
console.log(copy());
