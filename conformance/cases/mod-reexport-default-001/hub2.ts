// `default` as the SOURCE name: a named request for a binding
// literally called `default` would miss a's default export, so the
// nested load rides the default lane under the final name.
export { default as X } from "./a.ts";
export { default } from "./a.ts";
