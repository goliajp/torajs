// an arrow with a string parameter reached through `any`: an argument
// that is not a string runs the body under `any` (§13.3.2.1 [[Get]])
const f: any = (s: string) => s.length;
try {
  console.log(f(null));
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}
console.log(f("abc"));
console.log(f(42));
const g: any = (s: string) => s;
console.log(g(null), typeof g(7));

// the value reaches `any` through a typed binding
const k = (s: string) => s.length;
const f2: any = k;
try {
  console.log(f2(undefined));
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}
console.log(f2("hello"), k("hi"));

// a capture survives the hand-off
let base = 10;
const cap: any = (s: string) => base + s.length;
console.log(cap("ab"), cap(5));
base = 20;
console.log(cap("abc"));

// a closure only ever called through its typed face
const t = (s: string) => s.toUpperCase();
console.log(t("typed"));

// nested: the inner arrow escapes from a function
function make(prefix: string): any {
  return (s: string) => prefix + s.length;
}
const m = make("n=");
console.log(m("four"), m(3));
