// Type::Any-bound Set prints via AnyValue tag-walker → Tag::Set
// (=19, runtime Tag::Set substrate) → __torajs_set_print. Pre-split
// this returned `[object]` because Set heap blocks wore the same
// TAG_MAP=15 as Map and inspect.rs could not disambiguate.
const s: any = new Set();
console.log(s);
