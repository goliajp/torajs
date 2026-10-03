// A seeded loop accumulator left early, by return, throw or break,
// a million times over.
function byReturn(n: number): number {
  let a = '';
  if (n < 0) return -1;
  for (let i = 0; i < n; i++) a += 'x';
  return a.length;
}

function byThrow(n: number): number {
  let a = '';
  if (n < 0) throw new Error('negative');
  for (let i = 0; i < n; i++) a += 'x';
  return a.length;
}

function byBreak(n: number): number {
  let a = '';
  for (let i = 0; i < 100; i++) {
    if (n < 0) break;
    a += 'x';
  }
  return a.length;
}

const neg = -1 - Number(process.argv.length > 5 ? 1 : 0);
let sum = 0;
for (let k = 0; k < 1000000; k++) sum += byReturn(neg);
console.log(sum, byReturn(5));

let caught = 0;
for (let k = 0; k < 1000000; k++) {
  try {
    sum += byThrow(neg);
  } catch (e) {
    caught++;
  }
}
console.log(caught, byThrow(7));

sum = 0;
for (let k = 0; k < 1000000; k++) sum += byBreak(neg);
console.log(sum, byBreak(1));
