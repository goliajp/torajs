// §25.1.6 / §25.3.4 — ArrayBuffer.prototype owns byteLength /
// maxByteLength / resizable / detached, DataView.prototype owns buffer /
// byteLength / byteOffset; each getter throws on a receiver of any
// other brand, the prototype object itself included.
const ab: any = new ArrayBuffer(8, { maxByteLength: 16 });
const dv: any = new DataView(ab, 2, 4);
const ta: any = new Int8Array(4);
const rows: [any, string[], any][] = [
  [ArrayBuffer.prototype, ["byteLength", "maxByteLength", "resizable", "detached"], ab],
  [DataView.prototype, ["buffer", "byteLength", "byteOffset"], dv],
];
for (const [P, names, good] of rows) {
  for (const n of names) {
    const d: any = Object.getOwnPropertyDescriptor(P, n);
    const v = d.get.call(good);
    const bad: string[] = [];
    for (const r of [{}, [], ta, P, good === ab ? dv : ab, 1, undefined]) {
      try { d.get.call(r); bad.push("ok"); } catch (e: any) { bad.push(e.constructor.name[0]); }
    }
    console.log(n, typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length,
      v === ab ? "ab" : v, bad.join(""));
  }
}
console.log(JSON.stringify(Object.getOwnPropertyNames(DataView.prototype).filter((k: string) => k.startsWith("byte") || k === "buffer").sort()));
const P2: any = ArrayBuffer.prototype;
try { console.log(P2.byteLength); } catch (e: any) { console.log("proto read", e.constructor.name); }
const inh: any = Object.create(DataView.prototype);
try { console.log(inh.byteOffset); } catch (e: any) { console.log("inherited read", e.constructor.name); }
console.log(ab.byteLength, ab.detached, dv.byteLength, dv.buffer === ab);
