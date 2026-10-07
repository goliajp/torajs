// A typed array whose alias escaped into the any world holds whatever
// the alias stored; every builtin iteration hands its callback the
// element as it is (§10.2.1.3), not converted to the declared type.
const d: number[] = [1, 2, 3, 4];
const u: any = d;
u[0] = { k: 1 };
u[1] = "s";
u[2] = undefined;
console.log("map", d.map(v => typeof v).join(","));
d.forEach(v => console.log("forEach", typeof v, v));
console.log("filter", d.filter(v => typeof v === "string").length);
console.log("reduce", d.reduce((a, v) => a + typeof v, ""));
console.log("reduceRight", d.reduceRight((a, v) => a + typeof v, ""));
console.log("some", d.some(v => typeof v === "object"));
console.log("every", d.every(v => typeof v === "number"));
console.log("find", d.find(v => typeof v === "string"));
console.log("findIndex", d.findIndex(v => v === undefined));
console.log("findLast", d.findLast(v => typeof v === "number"));
console.log("findLastIndex", d.findLastIndex(v => typeof v === "object"));
console.log("from", Array.from(d, v => typeof v).join(","));
console.log("index", d.map((v, i) => typeof v + i).join(","));
console.log("source", d.filter((v, i, arr) => typeof v === "string" && arr.length === 4).length);
const arrow = (v: number) => typeof v;
console.log("bound", d.map(arrow).join(","));
const s: number[] = [3, 1, 2, 4];
const su: any = s;
su[1] = "s";
su[2] = { k: 1 };
const seen: string[] = [];
s.sort((a, b) => {
  seen.push(typeof a + "/" + typeof b);
  return String(a) < String(b) ? -1 : 1;
});
console.log("sort", seen.some(p => p.indexOf("object") >= 0), s.map(v => typeof v).join(","));
