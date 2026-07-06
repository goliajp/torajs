// lib for escape-any-002-xmod — exports a typed array whose alloc site
// must demote to Arr<Any> because the importing module aliases it into
// an any binding (module-level escape analysis crosses the import edge).
export const t: number[] = [10, 20];
