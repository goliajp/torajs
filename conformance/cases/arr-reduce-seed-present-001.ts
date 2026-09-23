// §23.1.3.24 / §23.1.3.25: without an initial value the seed is the first
// PRESENT element, and a hole is never visited.
const a: any[] = [, , 3, 4];
console.log(a.reduce((x: any, y: any) => x + y));
const b: any[] = [1, , 3];
console.log(b.reduce((x: any, y: any) => x + y), b.reduceRight((x: any, y: any) => x + y));
const c: any[] = [5, , ,];
console.log(c.reduceRight((x: any, y: any) => x * 10 + y));
try { ([, ,] as any[]).reduce((x: any, y: any) => x + y); } catch (e: any) { console.log(e instanceof TypeError); }
const d: any = [, 2, , 4];
console.log(d.reduce((x: any, y: any) => x + y), d.reduceRight((x: any, y: any) => x + y));
let visits = 0;
const e: any[] = [1, , 2];
e.reduce((x: any, y: any) => { visits++; return x + y; });
console.log(visits);
(Array.prototype as any)[1] = 10;
const f: any[] = [1, , 2];
console.log(f.reduce((x: any, y: any) => x + y));
