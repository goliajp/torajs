// §25.1.6.13 get ArrayBuffer.prototype.resizable — the reified
// accessor descriptor and its getter's whole reflection face.
const d: any = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "resizable");
console.log(typeof d, d && typeof d.get, d && d.set, d && d.enumerable, d && d.configurable);
console.log(d.get.name, d.get.length);
console.log(d.get.call(new ArrayBuffer(4)));
console.log(d.get.call(new ArrayBuffer(4, { maxByteLength: 8 })));
try { d.get.call({}); } catch (e) { console.log("throws"); }
try { (0, d.get)(); } catch (e) { console.log("throws2"); }
const ds: any = Object.getOwnPropertyDescriptor(Map.prototype, "size");
console.log(typeof ds.get, ds.get.name);
try { ArrayBuffer.prototype.resizable; console.log("no throw"); } catch (e) { console.log("direct throws"); }
const q: any = ArrayBuffer.prototype;
try { q.resizable; } catch (e) { console.log("indirect throws"); }
