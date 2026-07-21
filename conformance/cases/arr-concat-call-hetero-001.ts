// concat via any lane seeds an Any-kind product: spec arrays are
// heterogeneous, so a later string write must store, not raise the
// element-kind guard (rotation-176 sweep regression).
Object.defineProperty(Array.prototype, "0", { value: 100, writable: false, configurable: true });
const obj: any = Array.prototype.concat.call([101]);
const name: string = "0";
obj[name] = "unlikelyValue";
console.log(obj[name], obj[0]);
const d: any = Object.getOwnPropertyDescriptor(obj, "0");
console.log(d.value, d.writable, d.enumerable, d.configurable);
delete (Array.prototype as any)["0"];
const clean: any = [101];
clean[name] = "x";
console.log(clean[name]);
