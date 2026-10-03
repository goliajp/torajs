// Josephus survivor of `people` standing in a circle, every k-th one out,
// for every k up to an argv bound: the recurrence r = (r + k) % i.
function survivor(people: number, k: number): number {
  let r = 0;
  let i = 2;
  while (i <= people) {
    r = (r + k) % i;
    i++;
  }
  return r;
}

const limit: number = parseInt(process.argv[2], 10);
const people: number = parseInt(process.argv[3], 10);
let total: number = 0;
for (let k = 1; k <= limit; k++) {
  total += survivor(people, k);
}
console.log(total);
