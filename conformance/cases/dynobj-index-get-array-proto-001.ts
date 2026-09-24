// §10.1.8.1 OrdinaryGet walks the whole chain: an object whose prototype is
// an Array reads that array's elements by index.
function foo() {}
(foo as any).prototype = [1, 2, 3, 4];
var f: any = new (foo as any)();
console.log(f.length, f[0], 0 in f);
console.log(f.reduce((p: any, c: any) => p + c));
console.log(f.reduce((p: any, c: any) => p + c, -1));
console.log(f.reduceRight((p: any, c: any) => p + c, "4"));
console.log(f.map((x: any) => x * 2).join());
const oc: any = Object.create([7, 8]);
console.log(oc[1], oc.length);
