# past

`past` is a structural Python searcher and lightweight rule runner. It searches
the Python AST rather than raw text, making it suitable for local scripts,
pre-commit hooks, and CI checks.

## Build

```sh
cargo build --release
```

The binary is written to `target/release/past`.

## Structural Search

```sh
past find src 'call:eval'
past find src 'class:* -> function:regex:^[A-Z]'
past find src 'call:request AND argument:timeout:>30'
past find src 'function:* AND descendant(call:open) AND NOT descendant(call:close)'
past find src 'call:print AND ancestor(function:*)'
```

The first pattern is the node reported as the finding. Conditions inspect its
structure:

- `pattern` and `descendant(pattern)` search all nested nodes.
- `child(pattern)` searches direct AST children.
- `ancestor(pattern)` and `inside(pattern)` search enclosing nodes.
- `->` is shorthand for `AND descendant`.
- `AND`, `OR`, `NOT`, and parentheses compose conditions.

Supported node patterns:

```text
call:<value>
class:<value>
function:<value>
import:<value>
argument:<key>:<value>
```

Argument keys are keyword names, zero-based positional indexes, or `*`:

```text
argument:timeout:30
argument:0:"input.txt"
argument:*:None
```

Value predicates:

```text
*                         any value
User                      exact value
exact:User                exact value
contains:User             substring
starts_with:test_         prefix
ends_with:_unsafe         suffix
regex:^[A-Z]              regular expression
>3  >=3  <10  <=10        numeric comparison
```

Quote a complete query when invoking it from a shell. Quotes inside a value
allow spaces, for example `argument:0:"hello world"`.

## Automation

`find` returns `0` unless parsing or execution fails. Use `--fail-on-match` to
make matches return `1`:

```sh
past find . 'call:eval' --fail-on-match --quiet
```

Exit codes:

- `0`: successful and clean
- `1`: findings were detected when failure-on-match applies
- `2`: invalid query, configuration, or execution error

Output and filtering options:

```sh
past find . 'call:eval' --format json
past find . 'call:eval' --format jsonl
past find . 'call:eval' --include 'src/**/*.py' --exclude '**/generated/**'
past find . 'call:eval' --changed --max-matches 10
past find . 'call:eval' --no-cache
```

`--changed` includes staged, unstaged, and untracked Python files reported by
Git.

Directory searches store one content hash per file and findings per query or
rule in `.past-cache.json`. Unchanged files reuse cached findings. Changed files
are read, hashed, and parsed once, then all applicable rules run against the
same syntax tree. Use `--no-cache` to force a full scan. Cache failures fall
back to a full scan, and `--changed` or `--max-matches` searches do not use the
cache.

## Rule Files

Rules use TOML. See [`past.example.toml`](past.example.toml).

```toml
exclude = ["**/generated/**"]

[[rules]]
id = "no-eval"
query = "call:eval"
message = "Avoid eval(); parse the expected input explicitly."
severity = "error"
include = ["src/**/*.py"]
valid = ["parse(value)"]
invalid = ["eval(value)"]

[[rules]]
id = "method-name-case"
query = "class:* -> function:regex:^[A-Z]"
message = "Method names must start with a lowercase letter."
severity = "warning"
```

Run rules:

```sh
past check . --rules past.toml
past check . --rules past.toml --format json --changed
past test-rules --rules past.toml
```

`check` returns `1` when any rule matches. `test-rules` verifies that each
`valid` example does not match and each `invalid` example does.

## Suppressions

Suppress one line, the following line, or an entire file:

```python
eval(value)  # past: ignore no-eval

# past: ignore no-eval
eval(value)

# past: ignore-file no-eval
```

Omitting the rule ID suppresses every rule at that location. Multiple IDs can
be separated by spaces or commas.

## Pre-commit Example

```yaml
repos:
  - repo: local
    hooks:
      - id: past
        name: past structural rules
        entry: target/release/past check . --rules past.toml --changed
        language: system
        pass_filenames: false
```
