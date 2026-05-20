// Phase 1c.4.c: named capture group syntax `(?<name>X)` accepted by
// the parser; positional access (m[0] / m[N] / re.exec) still works.
// The .groups accessor is tested in regex-014.

// Single named group — positional access.
{
  const m = "foo123".match(/(?<digits>\d+)/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
}

// Two named groups.
{
  const m = "John Smith".match(/(?<first>\w+)\s(?<last>\w+)/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
  console.log(m === null ? "null" : m[2]);
}

// Named group with quantifier.
{
  const m = "aaa".match(/(?<a>a)+/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
}

// Named group + non-capturing.
{
  const m = "key=value".match(/(?:.*=)(?<v>.+)/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
}

// Named group with anchors.
console.log(/^(?<word>\w+)$/.test("hello"));
console.log(/^(?<word>\w+)$/.test("hello world"));

// .exec with named group — positional access still works.
{
  const r = /(?<key>\w+)=(?<val>\d+)/;
  const m = r.exec("foo=42");
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
  console.log(m === null ? "null" : m[2]);
}
