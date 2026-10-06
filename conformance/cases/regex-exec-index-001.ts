// The exec-shape properties of a match result (§22.2.7.2 steps
// 20-22) read through every spelling: plain member, optional chain,
// optional index, destructuring, and the groups object.

const re = /b(.)/g;
const m = re.exec("abcabd");
console.log(m !== null ? m.index : -1, m?.[1], m?.index, m?.input);
const m2 = re.exec("abcabd");
console.log(m2?.index, m2?.[1], re.lastIndex);
const m3 = re.exec("abcabd");
console.log(m3 === null, m3?.index, m3?.[1], re.lastIndex);

const miss = /q/.exec("abc");
console.log(miss?.index, miss?.input, miss?.groups, miss === null);

const st = /a/y;
st.lastIndex = 1;
const s1 = st.exec("aab");
console.log(s1?.index, s1?.[0], st.lastIndex);
const s2 = st.exec("aab");
console.log(s2 === null, s2?.index, st.lastIndex);

const n = "xyz".match(/y/);
console.log(n?.index, n?.input, n?.groups);
const nm = "xyz".match(/q/);
console.log(nm?.index, nm === null);

const kv = /(?<k>\w+)=(?<v>\d+)/.exec("x a=12");
if (kv !== null) {
  const [all, k, v] = kv;
  console.log(all, k, v, kv.index, kv.groups?.k, kv.groups!.v);
  const g = kv.groups;
  console.log(g === undefined ? "none" : g.k + g.v);
}
console.log("q=7".match(/(?<k>\w)=(?<v>\d)/)?.groups?.v);
console.log(/(\d)/.exec("ab3")?.groups);

const ms = [..."xbxb".matchAll(/b/g)].map((q) => q.index);
console.log(ms.join(","));
const parts: string[] = [];
for (const q of "a1b22".matchAll(/(?<d>\d+)/g)) {
  parts.push(`${q.index}:${q[0]}:${q.groups?.d}:${q.input}`);
}
console.log(parts.join(" "));
