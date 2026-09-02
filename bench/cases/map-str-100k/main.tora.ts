const m = new Map<string, number>();
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let j: number = i % 4096;
  let key: string = i % 2 === 0 ? "key" + j : "ключ" + j;
  if (m.has(key)) {
    m.set(key, m.get(key)! + i);
  } else {
    m.set(key, i);
  }
}
let total: number = 0;
for (const v of m.values()) {
  total = total + v;
}
console.log(m.size, total);
