// A body may fill several arrays in lockstep, and the pre-reserve
// install serves each one separately: `reserve(xs, len(xs) + bound)`
// per name. That is right only while the names are different arrays.
// Hand the same array in twice and the loop writes twice what either
// reservation covers — the stores run past the end of the buffer, and
// the per-name length write-back at the loop exit reports the count
// one name made rather than both. A refusal shows up as the right
// answer: 16, not 8.
function two_params(p: number[], q: number[]): number {
  for (let i: number = 0; i < 8; i++) {
    p.push(i);
    q.push(i * 10);
  }
  return p.length;
}
let a: number[] = [];
console.log(two_params(a, a));
console.log(a.join(","));

// Same shape on the while lane, which installs through the same gate.
function two_params_while(p: number[], q: number[]): number {
  let i: number = 0;
  while (i < 8) {
    p.push(i);
    q.push(i * 10);
    i = i + 1;
  }
  return p.length;
}
let b: number[] = [];
console.log(two_params_while(b, b));
console.log(b.join(","));

// One name needs no such proof — it is reserved against its own
// length, and nothing but this loop writes it while the loop runs.
function one_param(p: number[]): number {
  for (let i: number = 0; i < 8; i++) {
    p.push(i);
  }
  return p.length;
}
let c: number[] = [100];
console.log(one_param(c));
console.log(c.join(","));

// And the shape the multi-array install exists for: two arrays that
// really are two, still filled in lockstep at full speed.
let xs: number[] = [];
let ys: number[] = [];
for (let i: number = 0; i < 5; i++) {
  xs.push(i);
  ys.push(i * 10);
}
console.log(xs.length + " " + ys.length + " " + xs[4] + " " + ys[4]);
