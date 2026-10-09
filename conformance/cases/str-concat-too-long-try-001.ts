// A string `+` whose result would pass the longest string raises a
// RangeError that the enclosing try's catch receives, and nothing after
// the failed concat in the try body runs. The binding the failed
// assignment targets keeps the value it had. The one 1 GiB string is
// the only large allocation: a failing concat answers its left operand
// without copying it. Every fixture that needs a string that long
// lives in this file, so one test process at a time holds it.

const big = "x".repeat(2 ** 30);
let s = big;
let n = 0;
try {
  s = s + s;
  n++;
} catch (e) {
  console.log("+", e instanceof RangeError, n, s.length);
}

let after = 0;
try {
  s += s;
  after = 1;
} catch (e) {
  console.log("+=", e instanceof RangeError, after, s.length);
}

// the first `+` of the chain fails: the binding keeps its old value,
// not the old value with the rest of the chain on it
try {
  s = s + s + "y";
} catch (e) {
  console.log("chain", e instanceof RangeError, s.length);
}

s = "k";
try {
  s = big + big;
} catch (e) {
  console.log("other", e instanceof RangeError, s);
}

// a failed `+` throws before the rest of its chain is evaluated
let calls = 0;
function suffix(): string {
  calls++;
  return "z";
}
try {
  s = big + big + suffix();
} catch (e) {
  console.log("chain call", e instanceof RangeError, calls, s);
}

try {
  const t = `${big}${big}`;
  console.log("template built", t.length);
} catch (e) {
  console.log("template", e instanceof RangeError);
}

try {
  console.log("concat built", big.concat(big).length);
} catch (e) {
  console.log("concat", e instanceof RangeError);
}

function local(big: string): number {
  let laps = 0;
  for (let i = 0; i < 3; i++) {
    try {
      const r = big + big;
      laps += r.length;
    } catch (e) {
      laps += e instanceof RangeError ? 1 : 100;
    }
  }
  return laps;
}
console.log("local", local(big));

// A concat past the longest string in a loop raises on every lap: a
// loop that catches it inside its body catches it once per lap, and
// the loop-invariant concat built once in a preheader still does.
const tail = process.argv.length > 99 ? "z" : big;
const ring: string[] = ["", "", "", ""];

function keep(s: string, i: number): number {
  if (s.length === 7) throw new TypeError("seven");
  ring[i & 3] = s;
  return s.length;
}

let caught = 0;
let made = 0;
for (let i = 0; i < 5; i = i + 1) {
  try {
    const s = big + tail;
    made = made + keep(s, i);
  } catch (e) {
    caught = caught + (e instanceof RangeError ? 1 : 100);
  }
}
console.log("licm", caught, made, ring[0].length);

// a catch outside the loop: the first lap throws out of it
let laps = 0;
try {
  for (let i = 0; i < 5; i = i + 1) {
    const s = big + tail;
    laps = laps + keep(s, i);
  }
} catch (e) {
  console.log("licm outer", e instanceof RangeError, laps);
}

s = "";
console.log("done", "a" + "b", s.length);
