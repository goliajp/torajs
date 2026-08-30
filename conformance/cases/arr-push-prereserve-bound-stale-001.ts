// A bound that reads the length of the very array being filled. With
// the length word deferred to a register the cond reads a length that
// stopped moving, and the loop ends early: 15 where the answer is 19.
// With the word not deferred the trip count is right instead and the
// reservation is four elements short, so the unchecked stores run past
// the end of the buffer — same wrong reading, two ways out.
function f(): number {
  let xs: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  for (let i: number = 0; i < (xs.length >> 1); i++) { xs.push(i); }
  return xs.length;
}
function g(): number {
  let xs: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  for (let i: number = 0; i < (xs.length >> 1); i++) { xs.push(xs.length); }
  return xs.length;
}
console.log(f(), g());
