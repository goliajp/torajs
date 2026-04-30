function trial(i: number): number {
  try {
    throw i;
  } catch (e) {
    return e;
  }
}

let total: number = 0;
for (let i: number = 0; i < 100000; i = i + 1) {
  total = total + trial(i);
}
console.log(total);
