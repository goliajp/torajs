// `Map<K, V>` with a union key or value: a stored arrow takes its
// context from the right type argument, never from a member of the
// other argument's union.

function run(raw: any) {
  const m = new Map<number | (() => number), () => string>();
  m.set(1, () => "one " + raw.y);
  m.set(() => raw.y, () => "fn");
  m.forEach((v: () => string, k: number | (() => number)) =>
    console.log(typeof k === "number" ? k : k() + 1, v()),
  );
  const w = new Map<string | number, () => number>();
  w.set("a", () => raw.y);
  w.set(2, () => raw.z);
  w.forEach((v: () => number, k: string | number) => console.log(k, v() + 1));
  const st = new Set<number | (() => number)>();
  st.add(() => raw.z);
  st.add(4);
  st.forEach((e: number | (() => number)) => console.log(typeof e === "number" ? e : e() * 2));
}
run(JSON.parse('{"y":3.5,"z":-1.25}'));
