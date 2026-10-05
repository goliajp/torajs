// Paired trial divisions at their edges: an exit that makes a call
// before leaving (the lane vectors stay live across it in block
// order), a loop entered again after such an exit, dividends that are
// NaN, infinite, negative or fractional, a divisor counter that starts
// below zero and passes through it, and a counter read after the loop.

const k = parseInt(process.argv[2] ?? "0", 10);

// the exit prints before it leaves; the loop is entered again for
// every x, with the vectors of the last entry dead
function reportFirst(lo: number, hi: number, to: number): number {
  let hits = 0;
  for (let x = lo; x <= hi; x++) {
    let d = 2;
    for (; d <= to; d++) {
      if (x % d === 0) {
        console.log("hit", x, d);
        hits++;
        break;
      }
    }
    if (d > to) console.log("none", x, d);
  }
  return hits;
}

// the same over a fixed range, where the dividend and the divisor are
// known to be in the window the float quotient is exact in
function reportFirstFixed(): number {
  let hits = 0;
  for (let x = 59; x <= 66; x++) {
    let d = 2;
    for (; d <= 60; d++) {
      if (x % d === 0) {
        console.log("hit", x, d);
        hits++;
        break;
      }
    }
    if (d > 60) console.log("none", x, d);
  }
  return hits;
}

function bracketedFixed(): number {
  let s = 0;
  for (let x = 45; x < 54; x++) {
    console.log("enter", x);
    let i = 2;
    while (i * i <= x) {
      if (x % i === 0) {
        console.log("divides", x, i);
        break;
      }
      i = i + 1;
    }
    console.log("leave", x, i);
    s += i;
  }
  return s;
}

// a call in the block the loop is entered from, and one after it
function bracketed(n: number): number {
  let s = 0;
  for (let x = n; x < n + 6; x++) {
    console.log("enter", x);
    let i = 2;
    while (i * i <= x) {
      if (x % i === 0) break;
      i = i + 1;
    }
    console.log("leave", x, i);
    s += i;
  }
  return s;
}

// smallest divisor of x in [from, to], 0 when none; x and from are
// whatever the caller has: a NaN, infinite, negative or fractional
// dividend never divides evenly by the rule of the float remainder,
// and a divisor of 0 answers NaN
function smallestIn(x: number, from: number, to: number): number {
  for (let d = from; d <= to; d++) {
    if (x % d === 0) return d;
  }
  return 0;
}

function row(xs: number[], from: number, to: number): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) out += smallestIn(xs[i], from, to) + " ";
  return out;
}

console.log(reportFirst(59 + k, 66 + k, 60));
console.log(bracketed(47 + k));
console.log(reportFirstFixed(), bracketedFixed());
const odd = [NaN, Infinity, -Infinity, -12, -7, 7.5, -0, 0, 1, 12, 49, 9007199254740991, 9007199254740992];
console.log(row(odd, 2, 9));
console.log(row(odd, -3 + k, 4));
console.log(row(odd, -1 + k, 1));
console.log(row(odd, 0 + k, 0));
console.log(smallestIn(35, -6 + k, 6), smallestIn(-35, -6 + k, 6), smallestIn(35, 0 + k, 6), smallestIn(35, 1 + k, 6));
console.log(smallestIn(parseFloat("NaN"), 2, 50), smallestIn(parseFloat("Infinity"), 2, 50), smallestIn(parseFloat("36.5"), 2, 50));
