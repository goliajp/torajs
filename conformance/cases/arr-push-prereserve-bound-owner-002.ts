// "This name may denote a different array by the time the loop runs"
// is a question about the bodies that can write it, and only this
// body and the closures it builds can write its own bindings. Asking
// the whole program instead made a `src = …` in an unrelated function
// cost every `src` in the program its reservation.
function unrelated(): number {
  let src: number[] = [1];
  src = [2, 3];
  return src.length;
}

function own_literal(dst: number[]): number {
  let src: number[] = [0, 1, 2, 3, 4];
  for (let i: number = 0; i < src.length; i++) {
    dst.push(src[i] * 2);
  }
  return dst.length;
}
let out: number[] = [];
console.log(own_literal(out) + unrelated());
console.log(out.join(","));

// This body's own write still refuses, including from under a branch
// that may not even be taken: the analysis is static. Refusing shows
// up as 19; admitting would answer 15.
let shared: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
function get_shared(): number[] {
  return shared;
}
function own_write(flag: boolean): number {
  let dst: number[] = [];
  if (flag) {
    dst = get_shared();
  }
  for (let i: number = 0; i < (shared.length >> 1); i++) {
    dst.push(i);
  }
  return dst.length;
}
console.log(own_write(true));
console.log(own_write(false));
