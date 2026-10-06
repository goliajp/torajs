// An inner divide chain whose trip count depends on the outer counter:
// the lanes do not step in lockstep, so the jam keeps its narrow form
// and every lane still answers its own count.
function surv2(k: number): number {
  let r = 0;
  let i = 2;
  while (i <= (k % 9) + 3) {
    r = (r + k) % i;
    i++;
  }
  return r;
}
let t = 0;
for (let k = 0; k <= 200; k++) t += surv2(k) * (k + 1);
console.log(t);
function surv(n: number, k: number): number {
  let r = 0;
  for (let i = 2; i <= n; i++) r = (r + k) % i;
  return r;
}
let u = 0;
for (let k = 1; k <= 300; k++) u += surv(37, k) + surv(k % 50 + 2, k);
console.log(u);
