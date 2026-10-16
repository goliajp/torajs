// The RegExp protocol methods are named after their symbol keys.
const P: any = RegExp.prototype;
for (const k of ["match", "matchAll", "replace", "search", "split"]) {
  const f = P[(Symbol as any)[k]];
  const d = Object.getOwnPropertyDescriptor(f, "name")!;
  console.log(k, f.name, d.writable, d.enumerable, d.configurable);
}
console.log("".match.name, "".replace.name, P.exec.name);
