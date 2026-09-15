// A builtin prototype's `length` read through the any lane's `.length`
// fast kernel walks its builtin chain like `P["length"]` does: the
// %TypedArray.prototype% accessor throws on the prototype and on a
// per-kind prototype that inherits it; other prototypes keep their
// answers (Function.prototype 0, Array.prototype 0, Map.prototype none).
// String.prototype's own length 0 is a separate, pre-existing gap.
const TA: any = Object.getPrototypeOf(Int8Array);
const protos: any[] = [TA.prototype, Int8Array.prototype, Float64Array.prototype, Function.prototype, Array.prototype, Map.prototype, Object.prototype];
for (const p of protos) {
  let dot = "";
  let key = "";
  try { dot = String(p.length); } catch (e) { dot = "throw " + (e as any).constructor.name; }
  try { key = String(p["length"]); } catch (e) { key = "throw " + (e as any).constructor.name; }
  console.log(dot, key);
}
