// chunk 631 — FnSig-typed PARAM as replace/replaceAll callback: the
// param-tag pass gains a replace-cb usage axis, so the param retags
// __cls( and named-fn call-site args wrap in forwarders (617 residual
// — was a loud ssa-lower panic).
function up(m: string): string {
  return m.toUpperCase();
}
function g(cb: (s: string) => string): void {
  console.log("abcb".replace("b", cb));
  console.log("abcb".replaceAll("b", cb));
}
g(up);
g((s: string): string => "<" + s + ">");
function tag(m: string, pos: number, whole: string): string {
  return "[" + m + "@" + pos + "/" + whole.length + "]";
}
function h(cb: (m: string, pos: number, whole: string) => string): void {
  console.log("xyx".replace("x", cb));
  console.log("xyx".replaceAll("x", cb));
}
h(tag);
function both(cb: (s: string) => string): string {
  return cb("k") + "aca".replace("c", cb);
}
console.log(both(up));
