// a closure built in a generic body
function mk<T>(x: T): any {
  return (s: string) => s.length;
}
const fromGeneric = mk(1);
try {
  console.log(fromGeneric(null));
} catch (e: any) {
  console.log("generic null", e instanceof TypeError);
}
console.log(fromGeneric("four"), fromGeneric(7));

// a closure with an inner closure, both reached through any
const outer: any = (s: string) => {
  const inner: any = (t: string) => t.length;
  return inner(s) + inner(null === null ? s : s);
};
console.log(outer("ab"));
try {
  outer(null);
} catch (e: any) {
  console.log("outer null", e instanceof TypeError);
}

// typed array callbacks stay on the typed call
const words: string[] = ["aa", "bbb"];
console.log(words.map((w: string) => w.length).join(","));
console.log(words.filter((w: string) => w.length > 2).join(","));
console.log(words.some((w: string) => w === "aa"), words.findIndex((w: string) => w === "bbb"));
console.log(words.reduce((acc: number, w: string) => acc + w.length, 0));

// a replace callback against a regex known only at run time
const re: RegExp = new RegExp("(a)|b", "g");
try {
  console.log("ab".replace(re, (m: string, g: string) => m + g.length));
} catch (e: any) {
  console.log("runtime regex undefined group", e instanceof TypeError);
}
