// motivating bug: top-level labeled continue/break
outer: for (let i = 0; i < 3; i++) {
  for (let j = 0; j < 3; j++) {
    if (j === 1) continue outer;
    console.log(i, j);
  }
}
let r = "";
search: for (let i = 0; i < 5; i++) {
  for (let j = 0; j < 5; j++) {
    if (i * j > 6) { r += `break@${i},${j};`; break search; }
  }
}
console.log(r);
