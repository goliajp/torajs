// RFC 20260721-builtin-method-reflection 刀 7 (G7) — RegExp `/d`
// match-indices (MakeIndicesArray §22.2.7.8).

// basic: capture pairs in UTF-16 units + groups undefined
const m = /b(c)/d.exec("abcd")!;
console.log(m.indices[0]);
console.log(m.indices[1]);
console.log(m.indices.groups);
console.log(m.index, m.input);

// flag faces: hasIndices getter + flags string + toString
const re = /a/dg;
console.log(re.hasIndices, re.flags, re.toString());
console.log(/a/g.hasIndices);

// named groups: null-proto dict, pair object SHARED with indices[i]
const n = /(?<x>b)(c)/d.exec("abcd")!;
console.log(n.indices.groups.x);
console.log(n.indices.groups.x === n.indices[1]);

// non-participating group → undefined slot in indices AND groups
const p = /(z)?b/d.exec("abc")!;
console.log(p.indices[1]);
const q = /(?<w>z)?b/d.exec("abc")!;
console.log(q.indices.groups.w);

// duplicate named groups: participating twin wins
const dup = /(?:(?<z>c)|(?<z>d))/d.exec("xd")!;
console.log(dup.indices.groups.z);

// non-ASCII haystack: pairs are UTF-16 code units, not bytes
const u = /b/d.exec("αβb")!;
console.log(u.indices[0]);

// matchAll carries indices per entry; global match() (string array
// shape) has none
for (const mm of "ab ab".matchAll(/a(b)/dg)) {
  console.log(mm.indices[0], mm.indices[1]);
}
console.log("aa".match(/a/dg));

// without /d: no indices prop
console.log(/b/.exec("abc")!.indices);
