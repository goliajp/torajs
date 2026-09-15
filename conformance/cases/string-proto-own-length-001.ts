// §22.1.3 — %String.prototype% is a String exotic object over "", so it
// owns `length` 0 with writable / enumerable / configurable all false.
const SP: any = String.prototype;
console.log(typeof SP.length, SP.length, SP["length"], "length" in SP, Object.prototype.hasOwnProperty.call(SP, "length"));
const d: any = Object.getOwnPropertyDescriptor(SP, "length");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(Object.getOwnPropertyNames(SP).includes("length"), Object.keys(SP).length);
try {
  delete SP.length;
  console.log("deleted");
} catch (e: any) {
  console.log(e instanceof TypeError, SP.length);
}
try {
  SP.length = 5;
  console.log("wrote");
} catch (e: any) {
  console.log(e instanceof TypeError, SP.length);
}
const o: any = Object.create(String.prototype);
console.log(o.length, Object.prototype.hasOwnProperty.call(o, "length"));
const w: any = new String("abc");
const s: any = "xy";
const long: any = "a string long enough to not be a short immediate";
console.log(w.length, s.length, long.length, "abc".length, w["length"], s["length"]);
