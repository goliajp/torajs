// A union mixing literal types with other types (TS §3.2.10, §4.24):
// `typeof` narrows it, and a literal member reads its base type's
// members.

type Width = "auto" | "fit" | number;
type Flag = true | "yes" | 0;

function css(w: Width): string {
  if (typeof w === "string") {
    return w.toUpperCase() + ":" + w.length;
  }
  return w.toFixed(1) + "px";
}

function on(f: Flag): string {
  if (typeof f === "boolean") return "bool " + f;
  if (typeof f === "number") return "num " + (f + 1);
  return "str " + f.charAt(0);
}

const widths: Width[] = ["auto", 12, "fit", 0.5];
for (const w of widths) console.log(css(w));
console.log(on(true), on("yes"), on(0));
