function f(): number {
  let count = 0;
  loop: for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      if (j === 2) continue loop;
      count++;
    }
  }
  return count;
}
console.log(f());
const g = (): string => {
  let out = "";
  a: for (let i = 0; i < 3; i++) {
    b: for (let j = 0; j < 3; j++) {
      if (j === i) continue a;
      if (i + j === 3) break a;
      out += `${i}${j},`;
    }
  }
  return out;
};
console.log(g());
