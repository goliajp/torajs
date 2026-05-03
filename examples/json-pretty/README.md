# JSON serializer demo — torajs example

Serializes user-shaped class instances and primitive arrays via
`JSON.stringify`, and round-trips a literal back through `JSON.parse`
for type inference. Output is the compact (no-whitespace) JSON form.

## Running

```sh
tr run json-pretty.ts
# or compile to a native binary
tr build json-pretty.ts -o json-pretty && ./json-pretty
```

Expected output:

```
{"name":"Alice","age":30,"active":true,"tags":["admin","engineer"]}
{"name":"Bob","age":25,"active":false,"tags":["intern"]}
[1,2,3,5,8,13,21]
["alpha","beta","gamma"]
[10,20,30]
```

## Exercises

- Class declaration with mixed-type fields (number / string / boolean
  / `string[]`)
- Class instance → JSON via `JSON.stringify(instance)`
- Primitive `number[]` / `string[]` serialization
- Round-trip: `JSON.parse(text)` infers the result type from the
  caller's `let arr: number[] = ...` annotation, then re-emits via
  `JSON.stringify`

The 3-arg pretty-print form (`JSON.stringify(value, null, 2)`) is
parsed and typechecked but currently emits the same compact output as
the 1-arg form. Indent-aware emission lands in a follow-up SSA
lowering pass.

Output matches `bun run json-pretty.ts` exactly.
