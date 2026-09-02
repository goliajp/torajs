// A Map / Set key that is a Substr view finds the bucket its content
// owns. `split` hands out inline views into its receiver; a view of
// a UTF-16 receiver spells Latin-1 content in a wide payload, and
// hashing had read the owned-Str layout (a view's parent pointer and
// offset as payload, a UTF-16 key's units as half as many bytes)
// while equality already compared by code unit — so the key equal
// to a stored one still missed its bucket (rotation 560-01).
function tail(s: string): string { return s.slice(1); }
const m = new Map<string, number>();
m.set("abc", 1); m.set("ㄱ", 3); m.set("é", 5);
const parts = ["汉abc", "zabc", "xㄱ", "aé"];
console.log(m.get(tail(parts[0])), m.get(tail(parts[1])), m.get(tail(parts[2])), m.get(tail(parts[3])));
// Views of a UTF-16 receiver: "abc" spelled wide, "ㄱ", "é".
const sp = "汉,abc,ㄱ,é".split(",");
console.log(m.get(sp[1]), m.get(sp[2]), m.get(sp[3]), m.has(sp[1]));
// Views of a Latin-1 receiver.
const sp2 = "q,abc,é".split(",");
console.log(m.get(sp2[1]), m.has(sp2[1]), m.get(sp2[2]));
// "1" must not alias "ㄱ" (its low byte).
console.log(m.get("1"), m.has("1"));
// One entry per content, whichever cell spells it.
const st = new Set<string>();
st.add(sp[1]); st.add("abc"); st.add(tail(parts[0])); st.add(sp2[1]);
console.log(st.size, st.has(sp[1]), st.has("abc"));
// Set via a view, read via an owned key, and the other way round.
const m2 = new Map<string, number>();
m2.set(sp[1], 7); m2.set(tail(parts[0]), 8);
console.log(m2.size, m2.get("abc"), m2.get(sp[1]));
m2.set("abc", 9);
console.log(m2.size, m2.get(sp[1]), m2.delete(sp[1]), m2.size, m2.has("abc"));
// Any-typed keys take the same path.
const a: any = "汉,abc".split(",");
const m3 = new Map<any, any>(); m3.set(a[1], 9);
console.log(m3.get("abc"), m3.has(a[1]), m3.size, [...m3.keys()]);
