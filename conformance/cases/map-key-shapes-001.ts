// Map string keys reaching the lookup through different cell shapes:
// literals, concat results, substring views, UTF-16 payloads, and the
// string "undefined" next to the undefined key.
const m = new Map<any, number>();
const base = "prefix-alpha-beta";
m.set("alpha", 1);
m.set(base.slice(7, 12), 2); // a view spelling "alpha" — same key
m.set("ключ" + 7, 3);
m.set("undefined", 4);
m.set(undefined, 5);
m.set("", 6);
m.set("a".repeat(40), 7);
console.log(m.size, m.get("alpha"), m.get("ключ7"), m.get("ключ" + (14 / 2)));
console.log(m.get("undefined"), m.get(undefined), m.get(""), m.get("a".repeat(40)));
console.log(m.has(base.slice(7, 11)), m.has("alph" + "a"), m.has("ключ8"), m.has("AAAA".toLowerCase().repeat(10)));
const keys: string[] = [];
for (let i = 0; i < 20; i = i + 1) {
  const k = i % 2 === 0 ? "key" + (i % 6) : "ключ" + (i % 6);
  m.set(k, (m.get(k) ?? 0) + i);
  keys.push(k);
}
console.log(m.size, keys.join(","));
for (const [k, v] of m) {
  console.log(k, v);
}
