# Changelog

All notable changes to `torajs-meta` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release.
- `fnprops.rs` — side-table mapping function instances to property
  bags (for `fn.foo = "bar"; fn.foo` user-code patterns where
  closures need extra attached state beyond their environment
  capture).
- `classmeta.rs` — class-tag-keyed registries: class name, prototype
  object, instanceof check via prototype chain walk.
- `reflect.rs` — `Object.getOwnPropertyDescriptor` /
  `getPrototypeOf` / `setPrototypeOf` reflection ops.
