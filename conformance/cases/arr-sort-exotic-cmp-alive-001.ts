// A comparator reused after a sort over a holey array must still be alive.
let calls = 0;
const cmp = (x: any, y: any) => { calls++; return x - y; };
for (let r = 0; r < 200; r++) {
  const h: any[] = [5, , 3, 1];
  h.sort(cmp);
  const t: any[] = [{ v: r }, 2, 1];
  if (r === 199) console.log(h.length, h[0], h[1], h[2], 3 in h);
}
console.log([3, 1, 2].sort(cmp).join(), calls > 0);
