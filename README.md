# pyastq

`pyastq` is a structural Python searcher and lightweight rule runner. It searches
the Python AST rather than raw text, making it suitable for local scripts,
pre-commit hooks, and CI checks.

## Build

```sh
cargo build --release
```

The binary is written to `target/release/pyastq`.

## Install

`pyastq` is a Rust executable distributed as the `pyastq` Python package. Install
it as an isolated command-line tool:

```sh
uv tool install pyastq
# or
pipx install pyastq
```

For local development:

```sh
uv tool install .
pyastq --help
```

The PyPI package is only a distribution mechanism for the executable; it does
not provide an importable Python module.

### Versions

Release tags are the source of truth for published package versions. To publish
a new release:

1. Tag the commit, for example `git tag v0.2.0`.
2. Push the tag with `git push origin v0.2.0`.

The release workflow validates the tag, changes the package version in its
temporary checkout, builds wheels for Linux, macOS, and Windows, publishes them
to PyPI, and creates a GitHub release. `Cargo.toml` may therefore still show the
development version from the tagged commit.

An existing tag can be released from GitHub Actions by running the `Release`
workflow manually and entering the tag.

PyPI retains previously published versions, so users can select one explicitly:

```sh
uv tool install 'pyastq==0.1.0'
pipx install 'pyastq==0.1.0'
```

Uploading another build with the same version is not a replacement mechanism.
Fixes require a new version such as `0.1.1`.

## Structural Search

```sh
pyastq find src 'call:eval'
pyastq find src 'class:* -> function:regex:^[A-Z]'
pyastq find src 'call:request AND argument:timeout:>30'
pyastq find src 'function:* AND descendant(call:open) AND NOT descendant(call:close)'
pyastq find src 'call:print AND ancestor(function:*)'
pyastq find src 'call:$target' --var target=eval
pyastq find src 'call:eval AND argument:0:$expr' \
  --replace 'json.loads($expr)' --change-label 'replace eval'
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

### Query Variables

Use `$name` or `${name}` anywhere a complete value is accepted:

```sh
pyastq find src 'call:$target' --var target=requests.get
```

Defined variables are parsed as query values, so values such as `regex:^safe_`,
`contains:token`, or `>30` keep their normal meaning. Variables may also be used
inside value predicates:

```sh
pyastq find src 'function:regex:$pattern' --var pattern='^[A-Z]'
pyastq find src 'call:request AND argument:timeout:>$limit' --var limit=30
```

An undefined variable becomes a capture for one candidate match. Reusing the
same capture requires the same source value each time:

```sh
pyastq find src 'call:* AND argument:0:$x AND argument:1:$x'
```

That query reports calls whose first two positional arguments are the same.

### Changes

`find` can apply a labelled replacement to every matched AST node:

```sh
pyastq find src 'call:eval AND argument:0:$expr' \
  --replace 'json.loads($expr)' \
  --change-label 'replace eval with json.loads'
```

Replacement templates can use defined variables and captures. Use `$$` for a
literal dollar sign. Unsafe one-off changes must be labelled and explicitly
allowed:

```sh
pyastq find src 'call:eval AND argument:0:$expr' \
  --replace 'json.loads($expr)' \
  --change-label 'replace eval with json.loads' \
  --unsafe-change --allow-unsafe
```

### Python Name Resolution

Call patterns follow Python imports and aliases within each file:

```python
import requests as r
from requests import post as send

r.get(url)
send(url)
```

`call:requests.get` matches `r.get(url)`, and `call:requests.post` matches
`send(url)`. Resolution respects lexical scopes and known shadowing by
parameters, assignments, definitions, loop targets, and similar bindings.
Literal call matching remains available when no import relationship is known.

## Automation

`find` returns `0` unless parsing or execution fails. Use `--fail-on-match` to
make matches return `1`:

```sh
pyastq find . 'call:eval' --fail-on-match --quiet
```

Exit codes:

- `0`: successful and clean
- `1`: findings were detected when failure-on-match applies
- `2`: invalid query, configuration, or execution error

Output and filtering options:

```sh
pyastq find . 'call:eval' --format json
pyastq find . 'call:eval' --format jsonl
pyastq find . 'call:eval' --format sarif
pyastq find . 'call:eval' --include 'src/**/*.py' --exclude '**/generated/**'
pyastq find . 'call:eval' --changed --max-matches 10
pyastq find . 'call:eval' --no-cache
pyastq find . 'call:eval' --num-workers 4
pyastq find . 'call:$target' --var target=eval
pyastq find . 'call:eval AND argument:0:$expr' \
  --replace 'json.loads($expr)' --change-label 'replace eval'
```

`--changed` includes staged, unstaged, and untracked Python files reported by
Git.

Directory searches store one content hash per file and findings per query or
rule in `.pyastq-cache.json`. Unchanged files reuse cached findings. Changed files
are read, hashed, and parsed once, then all applicable rules run against the
same syntax tree. Use `--no-cache` to force a full scan. Cache failures fall
back to a full scan, and `--changed` or `--max-matches` searches do not use the
cache. File processing uses one worker by default; `--num-workers` enables
parallel processing.

## Rule Files

Rules use TOML. See the examples under [`examples/`](examples):

- [`examples/check/`](examples/check) demonstrates rules without replacements.
- [`examples/replace/`](examples/replace) demonstrates safe and unsafe
  replacements with expected output files.

```toml
exclude = ["**/generated/**"]
variables = { dangerous = "eval" }

[[rules]]
id = "no-eval"
query = "call:$dangerous AND argument:0:$expr"
message = "Avoid eval(); parse the expected input explicitly."
severity = "error"
change = { label = "replace eval with json.loads", replace = "json.loads($expr)", safety = "unsafe" }
include = ["src/**/*.py"]
valid = ["parse(value)"]
invalid = ["eval(value)"]

[[rules]]
id = "method-name-case"
query = "class:* -> function:regex:^[A-Z]"
message = "Method names must start with a lowercase letter."
severity = "warning"
```

Top-level rule variables apply to every rule. A rule may define its own
`variables = { ... }` table to override or add values for that rule only.
Variables that are not defined in TOML are treated as captures, just like
`find` queries.

Rules may also define a labelled `change`. Safe changes run with `check --fix`;
unsafe changes are skipped unless `--allow-unsafe` is also passed.

Run rules:

```sh
pyastq check . --rules pyastq.toml
pyastq check . --rules pyastq.toml --format json --changed
pyastq check . --rules pyastq.toml --fix
pyastq check . --rules pyastq.toml --fix --allow-unsafe
pyastq test-rules --rules pyastq.toml
```

Run the bundled check-only example:

```sh
pyastq check examples/check/example.py --rules examples/check/pyastq.toml
pyastq test-rules --rules examples/check/pyastq.toml
```

Run the bundled replacement example and compare it to the expected output:

```sh
cp examples/replace/example.py /tmp/pyastq-replace-example.py
pyastq check /tmp/pyastq-replace-example.py \
  --rules examples/replace/pyastq.toml --fix
diff -u examples/replace/expected.py /tmp/pyastq-replace-example.py
```

To apply unsafe replacements too:

```sh
cp examples/replace/example.py /tmp/pyastq-replace-example.py
pyastq check /tmp/pyastq-replace-example.py \
  --rules examples/replace/pyastq.toml --fix --allow-unsafe
diff -u examples/replace/expected-unsafe.py /tmp/pyastq-replace-example.py
```

`check` returns `1` when any rule matches. `test-rules` verifies that each
`valid` example does not match and each `invalid` example does.

Rules can also live in `pyproject.toml`:

```toml
[tool.pyastq]
exclude = ["generated/**", "migrations/**"]

[[tool.pyastq.rules]]
id = "no-eval"
query = "call:eval"
message = "Avoid eval(); parse the expected input explicitly."
severity = "error"
valid = ["parse(value)"]
invalid = ["eval(value)"]
```

Alternatively, reference a standalone rule file:

```toml
[tool.pyastq]
rules-file = "config/pyastq.toml"
exclude = ["build/**"]
```

`rules-file` is resolved relative to `pyproject.toml`. External and inline
rules may be used together: external rules are loaded first, inline rules are
appended, and exclusions from both configurations are combined. Rule IDs must
remain unique across both sources.

When `--rules` is omitted, `check` searches from the analyzed path toward the
filesystem root for a `pyproject.toml` containing `[tool.pyastq]`:

```sh
pyastq check .
pyastq test-rules
```

Passing `--rules` continues to support standalone rule files and explicit
`pyproject.toml` files:

```sh
pyastq check . --rules pyastq.toml
pyastq check . --rules pyproject.toml
```

SARIF 2.1.0 output is suitable for code-scanning systems:

```sh
pyastq check . --format sarif > pyastq.sarif
```

## Suppressions

Suppress one line, the following line, or an entire file:

```python
eval(value)  # pyastq: ignore no-eval

# pyastq: ignore no-eval
eval(value)

# pyastq: ignore-file no-eval
```

Omitting the rule ID suppresses every rule at that location. Multiple IDs can
be separated by spaces or commas.

## Pre-commit Example

```yaml
repos:
  - repo: local
    hooks:
      - id: pyastq
        name: pyastq structural rules
        entry: target/release/pyastq check . --rules pyastq.toml --changed
        language: system
        pass_filenames: false
```
