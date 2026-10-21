// a RegExp through a generator, an async fn and a thrown object
function* g() {
  yield /b/
}
for (const r of g()) console.log(r.test('b'))
async function f(): Promise<any> {
  return /a/
}
try {
  throw { re: /c/ }
} catch (e: any) {
  console.log(e.re.source)
}
f().then((r: any) => console.log(r.test('a')))
