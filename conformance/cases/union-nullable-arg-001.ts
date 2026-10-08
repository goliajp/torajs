// An optional value (`T | undefined`, `T | null`) passed into a union
// slot that has T and the nullish members (TS §3.11.4: each of its
// members fits one of the slot's).

function describe(v: string | number | undefined): string {
  if (v === undefined) return "none";
  return typeof v + ":" + v;
}

function fromOptional(s?: string): string {
  return describe(s);
}

function fromUndefinedUnion(n: number | undefined): string {
  return describe(n);
}

function withNull(v: string | boolean | null): string {
  return v === null ? "null" : typeof v + ":" + v;
}

function fromNullable(s: string | null): string {
  return withNull(s);
}

console.log(fromOptional("a"), fromOptional());
console.log(fromUndefinedUnion(3), fromUndefinedUnion(undefined));
console.log(fromNullable("x"), fromNullable(null), withNull(true));
