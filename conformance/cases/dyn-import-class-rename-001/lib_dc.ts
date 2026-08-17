// A dyn-import candidate whose exported class collides with an entry
// class: pre-knife-D the candidate was DROPPED outright (a class
// could not mangle); the census renames it now and the namespace
// field points at the mangled binding.
export class W {
  v() { return 3; }
}
export function w1() { return new W().v(); }
