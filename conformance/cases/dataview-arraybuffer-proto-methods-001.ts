// §25.3.4 / §25.1.6 — DataView.prototype's get/set methods and
// ArrayBuffer.prototype.transfer* are own methods of the prototype
// objects: readable as values, callable through .call, not
// constructors. They used to read undefined off the prototype.
const P: any = DataView.prototype;
console.log(typeof P.getInt8, typeof P.setFloat64, typeof P.getBigInt64, typeof P.setUint16);
console.log(Object.prototype.hasOwnProperty.call(P, "getUint32"), "setInt32" in P, P.getInt8.name, P.getInt8.length, P.setInt8.length);
const dv = new DataView(new ArrayBuffer(8));
P.setUint16.call(dv, 0, 513);
console.log(P.getUint16.call(dv, 0), dv.getUint8(0), P.getInt8 === (dv as any).getInt8);
try { new P.getInt8(); console.log("constructed"); } catch (e: any) { console.log(e.constructor === TypeError); }
const AB: any = ArrayBuffer.prototype;
console.log(typeof AB.transfer, typeof AB.transferToFixedLength, typeof AB.slice, AB.transfer.name);
const t = AB.transfer.call(new ArrayBuffer(4), 2);
console.log(t.byteLength);
