// 31 capture groups is the save-slot budget boundary
// (REGEX_SAVE_SLOTS = 64: whole-match pair + 31 group pairs);
// exec must return the full 32-element result. 32+ groups are a
// rejected subset boundary (abort, not silent-wrong) — not testable
// here since conformance expects bun byte-parity.
const re = /(a0)(a1)(a2)(a3)(a4)(a5)(a6)(a7)(a8)(a9)(a10)(a11)(a12)(a13)(a14)(a15)(a16)(a17)(a18)(a19)(a20)(a21)(a22)(a23)(a24)(a25)(a26)(a27)(a28)(a29)(a30)/;
const h: string = "a0a1a2a3a4a5a6a7a8a9a10a11a12a13a14a15a16a17a18a19a20a21a22a23a24a25a26a27a28a29a30";
const m = re.exec(h);
if (m === null) {
  console.log("null");
} else {
  console.log(m.length);
  console.log(m[0]);
  console.log(m[1]);
  console.log(m[31]);
  console.log(m.index);
}
