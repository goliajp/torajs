const arr: number[] = [1, 2];
console.log((0, arr).length);
console.log((0, arr)[1]);
const o = { p: 1, m() { return this.p; } };
console.log((0, o).p, (0, o).m());
(0, o).p = 5;
console.log(o.p);
(0, arr).push(3);
console.log(arr.length);
function late() {
  const it = { next() { log5.push("n"); return log5.length; } };
  const probe: any = {};
  probe.k = 1;
  const log5: string[] = [];
  return it.next();
}
console.log(late());
