// Type::Any-bound Map prints via AnyValue tag-walker → Tag::Map
// (=15) → __torajs_map_print. Pre-Tag::Set-substrate this routed
// through inspect.rs `[object]` fallback because Tag::Map covered
// BOTH Map and Set heap blocks (no Tag::Set existed; routing to
// __torajs_map_print on a Set heap would mis-print `Map(...)`).
const m: any = new Map();
console.log(m);
