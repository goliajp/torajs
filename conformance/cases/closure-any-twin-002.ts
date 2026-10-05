// struct, array and multi-parameter arrows reached through `any`
type P = { x: number; y: number };
const px: any = (p: P) => p.x + p.y;
console.log(px({ x: 1, y: 2 }));
console.log(px({ y: 5, x: 4 }));
console.log(px([1, 2]));
console.log(px("str"));
try {
  px(null);
} catch (e: any) {
  console.log("struct null", e instanceof TypeError);
}

const len: any = (xs: number[]) => xs.length;
console.log(len([1, 2, 3]));
console.log(len("abcd"));
console.log(len(["a", "b"]));
try {
  len(undefined);
} catch (e: any) {
  console.log("array undefined", e instanceof TypeError);
}

const first: any = (xs: string[]) => xs[0];
console.log(first(["p", "q"]), first([7, 8]), first("zy"));

// one slot of three does not fit
const mix: any = (a: string, b: number, c: string) => a + b + c.length;
console.log(mix("x", 1, "yy"));
console.log(mix("x", 2, 42));
try {
  mix("x", 3, null);
} catch (e: any) {
  console.log("third null", e instanceof TypeError);
}
console.log(mix(5, 1, "abc"));

// a callback stored in an any-typed container
const table: any = { go: (s: string, n: number) => s.repeat(n) };
console.log(table.go("ab", 2));
try {
  table.go(null, 2);
} catch (e: any) {
  console.log("method null", e instanceof TypeError);
}
const arr: any[] = [(s: string) => s.length];
console.log(arr[0]("xyz"), arr[0](1));

// a replace callback: a group that did not take part is undefined
const subj: any = "ab";
try {
  console.log(subj.replace(/(a)|b/g, (m: string, g: string) => m + g.length));
} catch (e: any) {
  console.log("replace undefined group", e instanceof TypeError);
}
console.log(subj.replace(/(a)|b/g, (m: string, g: string) => m + typeof g));
const typedSubj: string = "abab";
console.log(typedSubj.replace(/(a)|b/g, (m: string, g: string) => m + m));
