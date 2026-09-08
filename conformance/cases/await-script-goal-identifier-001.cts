// ES §13.1.1 — `await` is an ordinary BindingIdentifier and
// IdentifierReference under the Script goal. Only the Module goal
// reserves it; strictness has nothing to do with the word.
function useParam(await) {
  return await + 1;
}
console.log(useParam(41));

const selfNamed = (function await() {
  return "self-named";
})();
console.log(selfNamed);

function* gen() {
  var await = 7;
  return await * 2;
}
console.log(gen().next().value);

class Holder {
  static {
    // The grammar parameter is ON inside a static block and OFF again
    // inside a nested non-async function, so this class name is legal.
    const mk = () => {
      class await {}
      return typeof await;
    };
    console.log(mk());
  }
}
