// `A.length` is a loop invariant when `A` cannot be a second name for
// any array the loop fills. Either side can settle that, and only one
// of them used to be asked: every filled array being this body's own
// literal rules out any `A`, but so does `A` being this body's own
// literal — a cell made here was handed to nobody, so a parameter
// cannot be holding it. The second question is what lets a library
// function take the array to fill as a parameter.
function fill_from_own(dst: number[]): number {
  let src: number[] = [];
  for (let i: number = 0; i < 64; i++) {
    src.push(i * 3);
  }
  for (let i: number = 0; i < src.length; i++) {
    dst.push(src[i]);
  }
  return dst.length + dst[63];
}
let out: number[] = [7];
console.log(fill_from_own(out));
console.log(out[0] + " " + out[1] + " " + out[64]);

// Same on the while lane.
function fill_from_own_while(dst: number[]): number {
  let src: number[] = [1, 2, 3, 4, 5];
  let i: number = 0;
  while (i < src.length) {
    dst.push(src[i] * 10);
    i = i + 1;
  }
  return dst.length;
}
let out2: number[] = [];
console.log(fill_from_own_while(out2));
console.log(out2.join(","));

// Neither side owned. `src` is a literal this body wrote, but it was
// handed to a call before the loop, so it may now have a second name —
// and here it does: the callee stashed it, and `dst` is reassigned to
// it. Refusing shows up as 19, the count a per-iteration bound gives;
// admitting it would answer 15.
let g: number[] = [];
function stash(z: number[]): void {
  g = z;
}
function escaped_src(dst: number[]): number {
  let src: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  stash(src);
  dst = g;
  for (let i: number = 0; i < (src.length >> 1); i++) {
    dst.push(i);
  }
  return dst.length;
}
let out4: number[] = [];
console.log(escaped_src(out4));

// A string bound needs neither question: its length cannot move.
function fill_by_str(dst: number[], s: string): number {
  for (let i: number = 0; i < s.length; i++) {
    dst.push(i);
  }
  return dst.length;
}
let out3: number[] = [];
console.log(fill_by_str(out3, "hello"));
console.log(out3.join(","));
