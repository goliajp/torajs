// 558-05 — a string with a lone surrogate written to a file: there is
// no UTF-8 spelling for it, so the byte on disk is U+FFFD (bun writes
// `EF BF BD`, TextEncoder semantics); a paired surrogate is the
// 4-byte scalar, and a Latin-1 supplement char its 2-byte form.
// 559-06 — reading it back as `utf8` decodes those bytes into code
// units (before: the string was the raw bytes, `"é"` came back as
// `"Ã©"`).
import { writeFileSync, readFileSync, unlinkSync } from "fs";
const tmp = "/tmp/torajs_conf_fs_lone_surrogate.txt";
writeFileSync(tmp, "a\uD800b\uDE00c😀dé");
function units(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i).toString(16));
  return out.join(" ");
}
const back = readFileSync(tmp, "utf8");
console.log(back.length, units(back));
Bun.write(tmp, "\uDBFF" + "z");
console.log(units(readFileSync(tmp, "utf8")));
unlinkSync(tmp);
