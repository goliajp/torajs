// A loop guard of `i < xs.length` settles the upper bound of `xs[i]`
// and says nothing at all about the lower one. Eliding both compares
// on the strength of it read before the data pointer: with `i` walking
// up from a negative start, tr printed `0 3.0266414627e-314 10 20 30`
// where the answer is `undefined undefined 10 20 30` — a garbage heap
// read, silent, and invisible to every fixture that indexes forward.

const xs: number[] = [10, 20, 30];
let i: number = -2;
let out: string = "";
while (i < xs.length) {
  out = out + String(xs[i]) + " ";
  i = i + 1;
}
console.log(out);

// the same shape spelled as a for loop
let out2: string = "";
for (let j: number = -1; j < xs.length; j = j + 1) {
  out2 = out2 + String(xs[j]) + " ";
}
console.log(out2);

// a string element takes its own sentinel exit on the same path
const ss: string[] = ["a", "b"];
let out3: string = "";
let k: number = -1;
while (k < ss.length) {
  out3 = out3 + String(ss[k]) + " ";
  k = k + 1;
}
console.log(out3);

// the guarded window still elides the upper compare where it holds:
// walking forward from zero reads every element and never exits
let total: number = 0;
let n: number = 0;
while (n < xs.length) {
  total = total + xs[n];
  n = n + 1;
}
console.log(total);
