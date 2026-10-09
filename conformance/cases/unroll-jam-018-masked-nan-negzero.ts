// Masked lanes whose state turns NaN or -0: a NaN exit test that never
// fires, one written to fire on NaN, and -0 results that must reach the
// output with their sign.
function nanNeverExits(n: number): string {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    // lanes with i % 3 != 0 start where x * x overflows, so their step
    // gives NaN; the others shrink below the exit at their own step
    let x = (i % 3) * 1e200 - i * 0.5;
    let k = 0;
    while (k < 25) {
      if (x < -5) break;
      x = x * x - x * x + x * 1.5;
      k++;
    }
    out.push(k, x);
  }
  return out.join(",");
}
console.log(nanNeverExits(1));
console.log(nanNeverExits(9));

function nanExits(n: number): string {
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    let x = i + 0.25 + 0 / ((i % 4) - 1);
    let k = 0;
    while (k < 30) {
      if (!(x <= 1e6)) break;
      x = x * x * 0.5 + 0.5;
      k++;
    }
    out.push(k + ":" + x);
  }
  return out.join(" ");
}
console.log(nanExits(10));

function show(v: number): string {
  return Object.is(v, -0) ? "-0" : String(v);
}
function negZero(n: number): string {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    // even lanes start at 0 and stay there; y = -x keeps a -0
    let x = (i % 2) * i * 0.75;
    let y = 0;
    let k = 0;
    while (k < 12) {
      if (x > 20) break;
      y = x * -1;
      x = x * x * 0.5 + x;
      k++;
    }
    out.push(k, x, y);
  }
  return out.map(show).join(" ");
}
console.log(negZero(9));
