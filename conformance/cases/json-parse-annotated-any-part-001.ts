// JSON.parse into a slot whose annotated type has an `any` part below
// its top (`any[]`, a field typed `any`) reads every element and field.
const a: any[] = JSON.parse("[1,2,3]");
console.log(a[1], a.length, a);
const b: any[] = JSON.parse('[{"x":1},"s",null,[2]]');
console.log(b[0].x, b[1], b[2], b[3][0]);
const t = "[4,5]";
const c: any[] = JSON.parse(t);
console.log(c[0] + c[1]);
const d: Array<any> = JSON.parse("[7]");
console.log(d[0]);
let sum = 0;
for (const v of a) sum += v;
console.log(sum);
const out: number[] = [];
for (const v of a) out.push(v);
console.log(out.join(","));
type Env = { name: string; data: any };
const e: Env = JSON.parse('{"name":"n","data":{"k":[1,{"z":true}]}}');
console.log(e.name, e.data.k[1].z, JSON.stringify(e.data));
const rows: { id: number; extra: any }[] = JSON.parse('[{"id":1,"extra":"x"},{"id":2,"extra":3.5}]');
console.log(rows[0].extra, rows[1].extra + rows[1].id);
