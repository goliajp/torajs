// §6.2.6.5 accessor + data mix rejection — bun-parity message matrix
// ('value' wins over 'writable' when both present, matching JSC).
function t(mk: (d: any) => void, label: string) {
  const o: any = {};
  const d: any = {};
  mk(d);
  try { Object.defineProperty(o, "x", d); } catch (e: any) { console.log(label + ":", e.message); }
}
t(d => { d.get = function () { return 1; }; d.value = 2; }, "get+value");
t(d => { d.get = function () { return 1; }; d.writable = true; }, "get+writable");
t(d => { d.set = function (_v: any) {}; d.value = 2; }, "set+value");
t(d => { d.set = function (_v: any) {}; d.writable = false; }, "set+writable");
t(d => { d.get = function () { return 1; }; d.set = function (_v: any) {}; d.value = 2; d.writable = true; }, "all");
