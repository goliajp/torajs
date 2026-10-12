// a module that patches the global parser keeps the call on the
// dynamic lane, and the binding holds whatever the patch returns
(globalThis as any).parseFloat = (s: string): string => "p:" + s;
const n = parseFloat("1.5");
function show(): string {
  return "n=" + n;
}
console.log(show(), typeof n);
