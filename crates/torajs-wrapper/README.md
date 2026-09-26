# torajs-wrapper

Primitive-wrapper heap objects (`NumberWrapper` / `StringWrapper` /
`BooleanWrapper`) for the torajs AOT TypeScript runtime.

Implements the ES §21.1.1.1 / §22.1.1.1 / §20.3.1.1 `new
Number/String/Boolean(x)` heap substrate. Three fixed-size 16-byte
blocks share the universal [`torajs_rc::HeapHeader`] layout and
plug into `__torajs_value_drop_heap`'s tag-dispatch table.
