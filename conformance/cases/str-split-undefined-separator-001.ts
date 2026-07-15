// ES §22.1.3.21 String.prototype.split — separator === undefined.
// The 1-arg-undef guard was in place, but the 2+ arg shape fell
// through into str_split(recv, sep_null_slot) and SIGSEGV'd on the
// (Str, Str) ABI. Route 2+ arg with undef separator to
// str_split_no_sep, then respect the limit per spec steps 8-9
// (lim == 0 → [], else [S]).

const s = '--undefined--undefined--';

console.log(JSON.stringify(s.split(undefined, undefined)));
console.log(JSON.stringify(s.split(undefined, -1)));
console.log(JSON.stringify(s.split(undefined, 1)));
console.log(JSON.stringify(s.split(undefined, 0)));
console.log(JSON.stringify(s.split(undefined, 5)));
console.log(JSON.stringify(s.split("undefined", 1)));
console.log(JSON.stringify(s.split(undefined)));
console.log(JSON.stringify(s.split()));
