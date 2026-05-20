// Phase 1c.4.c: .groups dict accessor on match results — built from
// the regex's persisted capture_names table and attached via the
// arrprops side table, so `m.groups` resolves through the standard
// Array.<unknown-prop> lowering. `as any` cast is required because
// .match() / .exec() return type is Array<String>; the .groups
// property isn't surfaced in tora's RegExpMatchArray typing yet
// (L3b — typechecker uplift for RegExpMatchArray as a named type).

// Direct .groups.NAME access via `as any` cast.
{
  const m = "foo123".match(/(?<digits>\d+)/);
  if (m !== null) {
    const g = (m as any).groups;
    console.log(g.digits);
  }
}

// Two named groups, both accessible.
{
  const m = "John Doe".match(/(?<first>\w+)\s(?<last>\w+)/);
  if (m !== null) {
    const g = (m as any).groups;
    console.log(g.first);
    console.log(g.last);
  }
}

// .exec same shape.
{
  const r = /(?<key>\w+)=(?<val>\d+)/;
  const e = r.exec("foo=42");
  if (e !== null) {
    const g = (e as any).groups;
    console.log(g.key);
    console.log(g.val);
  }
}

// Non-participating named group → undefined per spec §22.2.5.7.
{
  const m = "abc".match(/(?<x>a)|(?<y>b)/);
  if (m !== null) {
    const g = (m as any).groups;
    console.log(typeof g.x);
    console.log(typeof g.y);
  }
}

// Named + positional captures coexist on the same result array.
{
  const m = "hello123world".match(/(?<word>\w+?)(\d+)/);
  if (m !== null) {
    console.log(m[0]);
    console.log(m[1]);
    console.log(m[2]);
    const g = (m as any).groups;
    console.log(g.word);
  }
}

// Mixed named groups + same backref.
{
  const m = "abcabc".match(/(?<rep>\w+)\k<rep>/);
  if (m !== null) {
    const g = (m as any).groups;
    console.log(g.rep);
  }
}

// .groups is only attached for named-group regexes.
{
  const m = "abc".match(/(\w+)/);
  if (m !== null) {
    const g = (m as any).groups;
    console.log(typeof g);
  }
}
