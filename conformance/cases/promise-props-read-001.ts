// rotation 353 — promise expando bag read channel (L3b ①): entries
// the defineProperty arm stores read back through the any member lane.
var p: any = Promise.resolve(1);
Object.defineProperty(p, "foo", { value: 42, writable: true, enumerable: false, configurable: true });
console.log("p1", p.foo);
console.log("p2", p.missing);
console.log("p3", typeof p.then);
async function main() {
  var v: any = await p;
  console.log("p4", v);
}
main();
