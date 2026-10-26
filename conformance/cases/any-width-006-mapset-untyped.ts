// A Map or Set built with no type arguments, or an any[], hands a stored
// arrow returning an any read back to a forEach callback that declares
// its function type; the call reads the value the arrow returned.

function run(raw: any) {
  const m = new Map();
  m.set("a", () => raw.y);
  m.forEach((k: () => number) => console.log(k() + 1));
  const keys = new Map();
  keys.set(() => raw.z, "k");
  keys.forEach((v: string, k: () => number) => console.log(v, k() * 2));
  const st = new Set();
  st.add(() => raw.y);
  st.forEach((k: () => number) => console.log(k() - 1));
  const arr: any[] = [() => raw.z];
  arr.forEach((k: () => number) => console.log(k() + 1));
}
run(JSON.parse('{"y":3.5,"z":-1.25}'));
