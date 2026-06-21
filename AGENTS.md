# Must-dos

- Use `cargo run -p wq-cli -- --help` to understand CLI usage
  - eg. `cargo run -p wq-cli -- exec 'inline code' -d ast,inst -p`
- Read `e/*.wq` to understand wq grammar
  - `lhs:rhs` is assignment
  - `a=b` is equality
  - list is `(1;2;3)`
    - wrong: `(1 2 3)`
  - call/index is `target[expr1;expr2...]`. Notice the brackets and semicolons
  - postfix:
    - `fn arg` calls
      - `fn1 fn2 arg` chains
      - wrong: `fn arg1 arg2`
    - `container index` indexes
  - `+` is broadcasting add
  - binary `,` is cat
  - leading `,` is enlist
  - `$[c;t;f]` is ternary. If false, every expression after the second semicolon belongs to the false branch, so `$[c;t;f1;f2]` runs `f1` then returns `f2` when `c` is false.
  - `$.[c;t1;t2...]` is a guard. It runs the body only when `c` is true; otherwise it returns unit `()`.
  - `$$[c1;t1;c2;t2;default]` is a condition/branch chain. Conditions are checked in order. The final default is optional; omitted default is unit.
  - `|` is pipe, which inserts lhs as the first arg to a rhs call
  - `\` or `bor[...]` (backslash) is bitwise or.
  - `\|` (backslash pipe) is short-circuit bool or.
  - `or[...]` is eager bool or.
  - `(1)` is not a list. It is simply atom `1`
  - `@r expr` is return
  - `@s <expr>` creates a symbolic CAS structure.
    - After using `@s` to create one, apply operations directly instead of stacking `@s`.
      - e.g. `diff integrate @s 1/(x^3-2)`
    - A bare `x^3-2` without `@s` is evaluation and is not related to CAS.
  - postfix binds tighter than operators, `echo 1+2` <=> `(echo 1)+2` => prints `1`, evals to `()/*unit*/+2` => evals to `2`. Does not evaluate to `3` and print `3`.
- Ensure `cargo clippy --all-targets -- -D warnings` passes.
  - Avoid using `#[allow(...)]` to pass clippy.
    - An exception is dead code that won't be used anymore, where you are allowed to use `#[allow(...)]` instead of deleting it
  - If you can't pass clippy by fixing code for any reason, ask the user whether it's fine to use `#[allow(...)]`
  - If passing clippy requires a large-scope edit, pause and ask the user.
- Do not run formatting commands.
- If you changed wq lexer/grammar, also update `wq-ts/grammar.js` and verify it with `tree-sitter generate` and a new corpse test
- Delevopment should be test-driven. Choose between unit tests and snapshot tests depending on situation.
  - Integration/snapshot tests use `hotchoco.py`.
    - This tests semantics, formatter, backtraces, etc.
    - `python3 hotchoco.py run`, when you touched:
      - lexer/parser/compiler/vm/interpreter
      - anything that affects semantics
    - If a new major module is added, you may create a new test config for it.
  - Key commands: `python3 hotchoco.py run`, `python3 hotchoco.py show --no-pager`, `python3 hotchoco.py accept`.
  - See `python3 hotchoco.py --help` for details.
- After a session, recommend a good commit message based on the appendix guidelines.
  - Do not commit unless the user explicitly asked for it.
- Unless the user explicitly requested, don't build/run with `release` profile.
- Prohibited without explicit user permission:
  - Python/Perl... scripts (especially regex-based replacements) for batch editing
  - `sed`, `awk`, or any similar text-processing utilities for code changes
  - `git checkout`, `git restore`, `git reset`, or any other destructive git mutations
  - `cargo clean`, or any other destructive cargo commands
  - Any cargo commands that trigger a complete rebuild
  - `rm` or any file deletion commands, except `trash`
- If you are given a perf-related task, prefer `hyperfine` over `time`
- Preferred approach: Make edits manually, one precise change at a time. If batch editing is unavoidable, ask the user for permission first.
  - When performing a batch edit, you must back up the target files first (eg. copy it as `.bak`) so it can be restored without git operations.
- When you are not sure of the user's intent, prefer asking the user instead of guessing.
- Avoid `panic!` outside tests. Prefer `unreachable!` or `debug_assert!` instead.
- Avoid `unwrap()`. Prefer `expect()`.
- Use `a.rs + a/` instead of `a/mod.rs` for modules with submodules.
  - Prefer no modifiers (i.e., private) over `pub(super)`.
  - Prefer `pub(super)` over `pub(crate)`.
  - Prefer `pub(crate)` over `pub`.
  - Avoid `pub` if it is not intended to be public API.
- If you touched Python code, run ruff.
- If you are using Playwright but chrome isn't available, use webkit.
- If you changed `wqpl/viz`, also update `wqide/viz`.
- Use these newer Rust features when they improve code style:

| Feature         | Stabilized in | Release date | Notes                                                                                                 |
| --------------- | ------------- | ------------ | ----------------------------------------------------------------------------------------------------- |
| let chains      | Rust 1.88.0   | 2025-06-26   | Stable only in Rust 2024 edition. Allows `if let ... && let ... && condition` and similar in `while`. |
| `if let` guards | Rust 1.95.0   | 2026-04-16   | Stabilizes `if let` guards on `match` arms, e.g. `pat if let Some(x) = expr => ...`.                  |

## Commit messages

- Clear, uncapitalized, imperative title: `fix everything`
- Avoid "conventional commit prefixes" (no `fix:`)
- Avoid trailing punctuation
- Optionally prefix the title with a crate name and colon when one crate is the clear scope: `wq-cli: did something`
- Clear, consise message body
- Include a `Release Notes:` section as the final section
- Use one bullet under `Release Notes:`:
  - `- Added ...`, `- Fixed ...`, or `- Improved ...` for user-facing changes, or
  - `- N/A` for docs-only and other non-user-facing changes.
- Format release notes exactly with a blank line after the heading, for example:
