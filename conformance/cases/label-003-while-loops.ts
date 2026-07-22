let i = 0;
let s = "";
w: while (i < 5) {
  i++;
  let k = 0;
  while (k < 5) {
    k++;
    if (k === 3) continue w;
    if (i === 4) break w;
    s += `${i}.${k} `;
  }
}
console.log(s.trim());
