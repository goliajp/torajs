// rotation 560 — the fn-name registry row points at the name's Str
// CELL, so a non-ASCII function name keeps its spelling on every
// face: inspect (`[Function: 函数]`), `.name`, the `bound ` marker,
// and the native-form toString. The pre-560 row pointed at the
// literal's payload bytes with no encoding, and a UTF-16 name
// printed as half its bytes.
function 函数() {}
const 箭头 = () => 1;
class C {
  方法() {}
}
console.log([函数, 箭头]);
console.log((函数 as any).name, 箭头.name, new C().方法.name);
const 绑 = 函数.bind(null);
console.log(绑.name, [绑]);
console.log(String(函数).startsWith("function "), String(绑));
function é() {}
console.log(é.name, [é]);
const o: any = { f: 函数 };
console.log(o.f.name, o);
