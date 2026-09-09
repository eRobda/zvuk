# Security Policy

## Supported versions

This project is pre-1.0. Only the latest commit on `main` receives fixes.

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Use
[private vulnerability reporting](https://github.com/eRobda/zvuk/security/advisories/new)
instead; it is visible only to the maintainers.

Expect an acknowledgement within a week.

## Scope

`zvuk` is a local command line tool. It opens audio devices, reads and writes
JSON files under a directory you choose, and makes no network connections at
all. The realistic attack surface is therefore small, but reports about the
following are welcome:

- crafted measurement JSON that causes a panic, an unbounded allocation, or a
  path traversal when read with `zvuk show`,
- a path supplied through `--out-dir` or `--label` escaping the intended
  directory,
- any use of `unsafe` that turns out to be unsound. The crate contains none of
  its own today.
