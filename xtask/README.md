# xtask

Repo automation. This is the only binary in this foundation workspace.

## Ownership

- Owns command dispatch and lint roots.
- Does **not** own lint rules — those live in `crates/structural-lint`.

## Usage

```
cargo xtask lint
```
