// Without the `u` flag the matcher still has to step whole
// characters: `.` means one character in either mode, and a negated
// class excludes whole code points. Stopping after a multi-byte
// character's first byte put the match boundary inside the
// character, and slicing there took the process down (exit 138).
const probes: string[] = [
  JSON.stringify("日X".match(/./)),
  JSON.stringify("é".match(/./)),
  JSON.stringify("日X".match(/[^X]/)),
  JSON.stringify("a日b".match(/[^ab]/)),
  JSON.stringify("a,日,b".match(/[^,]+/)),
  JSON.stringify("日本".match(/[^X]+/)),
  JSON.stringify("ab".match(/[^X]/)),
  JSON.stringify("日本".match(/\D/)),
  JSON.stringify("日本".match(/\w/)),
  JSON.stringify("日本語のテキスト".match(/テ[キク]スト/)),
  JSON.stringify("é".match(/[é]/)),
  JSON.stringify("é".match(/[\xE9]/)),
  JSON.stringify("é".match(/[é]/)),
  JSON.stringify("あ".match(/[ぁ-ん]/)),
  JSON.stringify("Ã".match(/[é]/)),
  JSON.stringify("a\nb".match(/a.b/)),
  JSON.stringify("a\nb".match(/a.b/s)),
  JSON.stringify("日X".match(/./u)),
  JSON.stringify("日X".match(/[^X]/u)),
];
for (const p of probes) console.log(p);
