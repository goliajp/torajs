// Function.prototype own name/length reflection (§20.2.3) — the
// %Function.prototype% intrinsic owns the fn meta pair as data
// props {writable: false, enumerable: false, configurable: true};
// strict assign throws, delete removes, defineProperty restores.
const F: any = Function.prototype;
for (const k of ["length", "name"]) {
  const d = Object.getOwnPropertyDescriptor(F, k);
  console.log(k, JSON.stringify(d.value), d.writable, d.enumerable, d.configurable,
    F.hasOwnProperty(k), Object.hasOwn(F, k), F[k] === d.value);
}
try { F.name = "x"; } catch (e: any) { console.log("w:", e instanceof TypeError, F.name === ""); }
delete F.length;
console.log("del:", F.hasOwnProperty("length"), Object.getOwnPropertyDescriptor(F, "length") === undefined);
Object.defineProperty(F, "length", { value: 0, writable: false, enumerable: false, configurable: true });
console.log("restored:", F.hasOwnProperty("length"), F.length);
