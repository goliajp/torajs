// Nested-print substrate trunk Commit 6 acceptance — Tag::RegExp
// typed-receiver console.log. SSA dispatcher Type::RegExp →
// print_any → __torajs_print_anyv Tag::RegExp branch →
// __torajs_regex_print_inline (reads RegExp::src_bytes + decodes
// RegExp::flags bitmask in spec emission order gimsuy). Closes
// RegExp typed-receiver wedge.
const r: RegExp = /abc/g;
console.log(r);
const r2: RegExp = /hello.*world/im;
console.log(r2);
const r3: RegExp = /^foo$/;
console.log(r3);
