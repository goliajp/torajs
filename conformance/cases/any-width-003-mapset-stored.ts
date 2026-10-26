// An arrow handed to Map.set or Set.add is stored with the receiver's
// declared function type; one that returns an `any` read is called back
// through that signature by forEach and keeps its fraction.

function run(raw: any) {
  const m = new Map<string, () => number>();
  m.set("a", () => raw.y);
  m.forEach((k: () => number) => console.log(k() + 1));
  const g = m.get("a");
  if (g) console.log(g() + 1);
  const keys = new Map<() => number, string>();
  keys.set(() => raw.y, "k");
  keys.forEach((v: string, k: () => number) => console.log(v, k() + 1));
  const st = new Set<() => number>();
  st.add(() => raw.z);
  st.forEach((k: () => number) => console.log(k() * 2));
}
run(JSON.parse('{"y":3.5,"z":-1.25}'));
