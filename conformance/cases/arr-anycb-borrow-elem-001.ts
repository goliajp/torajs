// HO-callback over typed refcounted elems with an any-param cb must
// not steal the array element stake (rotation 184 — chunk 753 made
// box_to_any a pure encode; the unconditional post-call drop then
// freed live elements, read back as garbage after heap churn).
const objs = [{ a: 111 }, { a: 222 }];
objs.find((x: any) => false);
objs.some((x: any) => false);
objs.every((x: any) => true);
const junk: any[] = [];
for (let i = 0; i < 5000; i++) junk.push({ z: i });
console.log(objs[0].a, objs[1].a);
const hit = objs.find((x: any) => x.a === 222);
console.log(hit.a);
console.log(objs[0].a, objs[1].a);
