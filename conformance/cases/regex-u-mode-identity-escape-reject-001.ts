// ES §22.2.1.1 IdentityEscape — in u/v mode, the char after `\`
// must be a SyntaxCharacter (`^ $ \ . * + ? ( ) [ ] { } |`) or
// `/`. Other identity escapes are early errors (SyntaxError). Non-u
// mode keeps the annexB ExtendedPatternCharacter lenience (treat as
// a literal). Pre-fix tr accepted `\q` / `\z` / etc. under u.

// u-mode rejects
const badU = ["\\q", "\\z", "\\%", "\\@"];
for (const p of badU) {
  try {
    new RegExp(p, "u");
    console.log(p, "u no-throw (bug)");
  } catch (e: any) {
    console.log(p, "u:", e.name);
  }
}

// v-mode rejects too
try { new RegExp("\\q", "v"); console.log("v no-throw"); } catch (e: any) { console.log("v:", e.name); }

// annexB non-u still accepts (literal)
console.log("annex q:", new RegExp("\\q", "").test("q"));
console.log("annex z:", new RegExp("\\z", "").test("z"));
console.log("annex source:", new RegExp("\\q", "").source);

// SyntaxCharacters + `/` under u — all valid
const okU = ["\\.", "\\*", "\\+", "\\?", "\\(", "\\)", "\\[", "\\]", "\\{", "\\}", "\\|", "\\/", "\\^", "\\$", "\\\\"];
let okCount = 0;
for (const p of okU) {
  try { new RegExp(p, "u"); okCount++; } catch {}
}
console.log("valid-u-count:", okCount);
