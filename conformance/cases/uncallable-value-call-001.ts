// Callees whose static type is a known-uncallable VALUE throw the
// §13.3.6.2 runtime TypeError instead of stopping the compile —
// arguments still evaluate first (step 4).
let n = 0;
function arg(): number {
  n++;
  return n;
}
try {
  // @ts-ignore
  true();
} catch (e) {
  console.log("bool", e instanceof TypeError);
}
const x: number = 1;
try {
  // @ts-ignore
  x(arg());
} catch (e) {
  console.log("num", e instanceof TypeError);
}
console.log("args", n);
function f(): void {}
try {
  // @ts-ignore
  f.length();
} catch (e) {
  console.log("len", e instanceof TypeError);
}
try {
  // @ts-ignore
  /abc/();
} catch (e) {
  console.log("re", e instanceof TypeError);
}
try {
  // @ts-ignore
  undefined();
} catch (e) {
  console.log("undef", e instanceof TypeError);
}
try {
  // @ts-ignore
  null();
} catch (e) {
  console.log("null", e instanceof TypeError);
}
const s: string = "hi";
try {
  // @ts-ignore
  s();
} catch (e) {
  console.log("str", e instanceof TypeError);
}
