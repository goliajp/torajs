# torajs-fs

[![Crates.io](https://img.shields.io/crates/v/torajs-fs?style=flat-square&logo=rust)](https://crates.io/crates/torajs-fs)
[![docs.rs](https://img.shields.io/docsrs/torajs-fs?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-fs)
[![License](https://img.shields.io/crates/l/torajs-fs?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-fs?style=flat-square)](https://crates.io/crates/torajs-fs)

Synchronous filesystem operations for the [torajs] AOT TypeScript
runtime — the `fs.*Sync` family. 0 Cargo deps (uses libc directly).

Extracted from `runtime_str.c`'s `fs_*` family (~340 LOC) as **P7.d**
(commit `88c21c4`, 2026-05-24). Path bytes flow through tora `Str`;
bodies returned as fresh `Str` heap blocks.

## Surface (Node.js-compatible subset)

| TS / Node API | extern `__torajs_fs_*` symbol | Notes |
| --- | --- | --- |
| `readFileSync(path)` | `__torajs_fs_read_file_sync` | Returns body as Str; throws on ENOENT |
| `writeFileSync(path, body)` | `__torajs_fs_write_file_sync` | Creates/truncates the file |
| `appendFileSync(path, body)` | `__torajs_fs_append_file_sync` | Appends to existing or creates |
| `existsSync(path)` | `__torajs_fs_exists_sync` | bool via i64; 1 = exists |
| `mkdirSync(path, opts)` | `__torajs_fs_mkdir_sync` | recursive bit in opts |
| `unlinkSync(path)` | `__torajs_fs_unlink_sync` | Removes a file |
| `statSync(path).size` | `__torajs_fs_stat_size_sync` | Returns i64 size; -1 on ENOENT |
| `readdirSync(path)` | `__torajs_fs_readdir_sync` | Returns Array<Str> of entry names |

## What it does NOT do (v0.1.0)

- **Async / Promise-based variants** (`fs.promises.readFile`): out of
  scope until threading lands; the sync surface covers the common
  CLI / script use case.
- **Streaming**: `createReadStream` / `createWriteStream` — out.
- **File modes / permissions**: no `chmod` / `chown` / `fstat`-with-
  full-stat. Just size.
- **Encoding**: bodies are raw bytes through Str; UTF-8 validation
  is the caller's job (per Node's default 'buffer' encoding shape).
- **Symbolic-link traversal opts**: `lstatSync` / `realpathSync` —
  add when a caller needs them.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
