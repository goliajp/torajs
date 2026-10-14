// A generic class's field whose type names none of its type parameters
// is laid out alike in every instance: a value written into it through
// the any lane, and a value cast into the instance, read through that
// field as they would in a class of its own.

type Inner = { a?: number; b: number };
class W<T> {
  inner: Inner | null = null;
  t: T | null = null;
}

const w = new W<number>();
(w as any).inner = JSON.parse('{"a":null,"b":2}');
const a = w.inner!.a;
console.log(a, a === null, a === undefined, typeof a, w.inner!.b);
console.log(JSON.stringify(w.inner));

const w2 = new W<string>();
(w2 as any).inner = JSON.parse('{"b":5}');
console.log(w2.inner!.a, w2.inner!.b, "a" in w2.inner!);

class Box {
  a: number | null = 1;
  b: number = 0;
}
class H<T> {
  box: Box | null = null;
  tag: T | null = null;
}
const h = new H<boolean>();
(h as any).box = JSON.parse('{"a":null,"b":4}');
console.log(h.box!.a, h.box!.b, typeof h.box!.a);

function show(label: string, f: () => string): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "threw", (e as Error).name);
  }
}
show("full", () => {
  const v: W<string> = JSON.parse('{"inner":{"a":1,"b":3},"t":"q"}');
  return `${v.inner!.a} ${v.inner!.b} ${v.t}`;
});
show("lacking", () => {
  const v: W<string> = JSON.parse('{"inner":{"b":3},"t":"q"}');
  return `${v.inner!.a} ${v.inner!.b} ${v.t}`;
});
show("null inner", () => {
  const v: W<string> = JSON.parse('{"inner":null,"t":"q"}');
  return `${v.inner} ${v.t}`;
});
show("null member", () => {
  const v: W<number> = JSON.parse('{"inner":{"a":null,"b":3},"t":7}');
  return `${v.inner!.a} ${v.inner!.b} ${v.t}`;
});
