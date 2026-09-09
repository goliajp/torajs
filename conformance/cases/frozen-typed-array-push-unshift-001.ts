// The TYPED lane of the push/unshift [[Extensible]] rule — number[] /
// string[] receivers, whose kernels take the raw slot value and whose
// lowering incs the array's stake only after the call. So the kernel
// refuses without releasing anything, and the lowering's throw check
// right after the call settles the value's own stake on the throw edge
// (parked on throw_live for the call's duration) exactly as the normal
// edge does after the inc. Before this, a frozen number[]'s push(9)
// appended through the freeze and answered the new length.
function t(tag: string, f: () => unknown) {
  try {
    const r = f();
    console.log(tag, "NO-THROW ->", r);
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
const n: number[] = [1, 2, 3];
Object.freeze(n);
t("num push", () => n.push(9));
t("num unshift", () => n.unshift(9));
console.log("num ->", JSON.stringify(n));

const s: string[] = ["a"];
Object.freeze(s);
t("str push literal", () => s.push("b"));
t("str unshift literal", () => s.unshift("b"));
// Owned-shape args — a call result and a fresh Substr — exercise the
// throw edge's release of the value's own stake.
function mk(): string {
  return "x".repeat(3);
}
t("str push call-result", () => s.push(mk()));
t("str unshift call-result", () => s.unshift(mk()));
const src = "hello";
t("str push substr", () => s.push(src[1]));
t("str unshift substr", () => s.unshift(src[1]));
console.log("str ->", JSON.stringify(s));

// Field receiver (`obj.items.push`) and a sealed / preventExtensions
// receiver — same rule as the any-side doors, read off node.
class Box {
  items: number[] = [1];
}
const b = new Box();
Object.freeze(b.items);
t("field push", () => b.items.push(2));
console.log("field ->", JSON.stringify(b.items));

const sealed: number[] = [1];
Object.seal(sealed);
t("sealed push", () => sealed.push(2));
const ne: number[] = [1];
Object.preventExtensions(ne);
t("ne unshift", () => ne.unshift(0));

// An extensible receiver still grows (the guard's fast path), and the
// return value is still the new length.
const ok: number[] = [1];
console.log("extensible push ->", ok.push(2), ok.unshift(0), JSON.stringify(ok));
