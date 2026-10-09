// Inner loops left early whose step has an effect (a call, a global
// write, an array write, a log): lanes must not run ahead, so the outer
// loop keeps its original order of effects.
let calls = 0;
function bump(x: number): number {
  calls++;
  return x * 0.5;
}
function withCall(n: number): number {
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.75;
    let k = 0;
    while (k < 40) {
      if (x > 30) break;
      x = x * x * 0.5 + bump(x);
      k++;
    }
    total = total * 3 + k;
  }
  return total;
}
console.log(withCall(11), calls);

let writes = 0;
function withGlobal(n: number): number {
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.75;
    let k = 0;
    while (k < 40) {
      if (x > 30) break;
      x = x * x * 0.5 + 1;
      writes = writes * 7 + k;
      if (writes > 1e9) writes = writes % 1009;
      k++;
    }
    total = total * 3 + k;
  }
  return total;
}
console.log(withGlobal(11), writes);

const trace: number[] = [];
function withArray(n: number): number {
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.75;
    let k = 0;
    while (k < 40) {
      if (x > 30) break;
      x = x * x * 0.5 + 1;
      trace[k] = i * 100 + k;
      k++;
    }
    total = total * 3 + k;
  }
  return total;
}
console.log(withArray(11), trace.join(" "));

function withLog(n: number): number {
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.75;
    let k = 0;
    while (k < 40) {
      if (x > 30) break;
      x = x * x * 0.5 + 1;
      console.log("step", i, k, x);
      k++;
    }
    total = total * 3 + k;
  }
  return total;
}
console.log(withLog(9));
