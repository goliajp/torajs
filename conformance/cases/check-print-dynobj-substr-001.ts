// W-M-rest follow-up — dynobj-stored Substr round-trips through
// console.log/print_anyv. Pre-fix Tag::Str dispatch in
// __torajs_print_anyv treated every Tag::Str heap block as a
// Str-layout (data@+16) and garbled Substr's parent-ptr@+16. The fix
// adds FLAG_SUBSTR_VIEW (bit 2 of HeapHeader::flags) — set by
// __torajs_substr_create and split-tail emit — so print_anyv can
// dispatch to __torajs_substr_print instead of __torajs_str_print.
//
// 3 Substr-producing paths: String.prototype.charAt / .trim / .slice.
// Bun parity verified byte-equal.

const s = "hello";
const o: any = {};
o.a = s.charAt(2);
console.log(o.a);

o.b = "  hi  ".trim();
console.log(o.b);

o.c = "hello".slice(1, 4);
console.log(o.c);
