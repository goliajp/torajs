// An async function settles its promise itself (§27.7.5.2
// AsyncBlockStart, §27.2.1.3.2): a script that replaces
// `Promise.resolve` is never consulted, including when the returned
// value reads `arguments`.
const orig = Promise.resolve;
(Promise as any).resolve = (v: any) => {
  console.log("user resolve called", v);
  return orig.call(Promise, v);
};
async function plain(x: number) {
  return x + 1;
}
async function args(x: number) {
  return arguments.length + x;
}
async function spread(...xs: number[]) {
  return [...arguments].length + xs.length;
}
async function counted(a: any, b: any) {
  return arguments.length;
}
plain(1).then((v) => console.log("plain", v));
args(1).then((v) => console.log("args", v));
(args as any)(1, 2, 3).then((v: any) => console.log("args3", v));
spread(1, 2).then((v) => console.log("spread", v));
(counted as any)(1).then((v: any) => console.log("counted", v));
