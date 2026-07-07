// chunk 632 — backward param marking: a fn-typed param passed into an
// already-marked (env-first) param slot must carry the closure shape
// too, and named-fn args at ITS call sites wrap in forwarders (was a
// silent crash: FnSig value handed to a slot read as a closure env).
function up(s: string): string {
  return s.toUpperCase();
}
function g3(cb: (s: string) => string): void {
  console.log(cb("x"));
}
function h(cb2: (s: string) => string): void {
  g3(cb2);
}
g3((s: string): string => s + "!");
h(up);
// two-level chain: outer -> mid -> g3, named fn enters at the top
function mid(a: (s: string) => string): void {
  g3(a);
}
function outer(b: (s: string) => string): void {
  mid(b);
}
outer(up);
outer((s: string): string => s + "?");
// replace-cb axis (chunk 631) as the marking source of the chain
function r(cb: (s: string) => string): void {
  console.log("aba".replace("b", cb));
}
function viaR(cb2: (s: string) => string): void {
  r(cb2);
}
viaR(up);
