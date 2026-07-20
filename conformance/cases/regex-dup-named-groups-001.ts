// RFC 20260721-string-proto-cluster 刀 7 (G8) — duplicate named
// groups: the non-participating twin must not overwrite the matched
// alternative's captured value in `.groups`, and key order follows
// the first source occurrence (§22.2.7.8).
const m1: any = "abc".match(/(?:(?<x>a)|(?<y>a)(?<x>b))(?:(?<z>c)|(?<z>d))/);
console.log(m1.groups.x, m1.groups.y, m1.groups.z);
console.log(Object.keys(m1.groups).join(","));
const m2: any = "ad".match(/(?:(?<x>a)|(?<y>a)(?<x>b))(?:(?<z>c)|(?<z>d))/);
console.log(m2.groups.x, m2.groups.y, m2.groups.z);
console.log(Object.keys(m2.groups).join(","));
const m3: any = "abd".match(/(?:(?<z>c)|(?<z>d))/);
console.log(m3.groups.z);
