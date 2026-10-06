// A typed array handed to the any world through a cast expression
// (`xs as any`) takes kind-changing pushes like any other alias; typed
// reads of the same binding see the new elements.
const c: number[] = [3, 1, 2];
(c as any).push(undefined);
console.log(c.length, c[3]);
c.sort((x, y) => x - y);
console.log(c);

const f: number[] = [3, 1.5, 2];
(f as any).push("s");
console.log(f.length, f[1], f[3]);
console.log(f);

const o: number[] = [7];
(o as any).push({ k: 1 }, [2, 3], null);
console.log(o.length);
console.log(o);
let sum = 0;
for (const v of o) if (typeof v === "number") sum += v;
console.log(sum);

const s: string[] = ["b", "a"];
(s as any).push(7);
console.log(s.length, s[0], s[2]);
console.log(s.join("-"));
