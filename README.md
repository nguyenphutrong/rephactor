# Rephactor

Rephactor is a companion PHP language server for semantic refactoring code
actions. The first refactor is adding PHP 8 named argument identifiers to
positional call arguments.

The intended editor setup is to keep a full PHP language server such as
Intelephense or Phpactor for completion, diagnostics, hover, and navigation,
then run this server alongside it for a narrow code action:

```php
dispatch($order, true);
```

```php
dispatch(order: $order, notify: true);
```

This project starts intentionally small. It should not become a full PHP
language server until the named-arguments workflow is correct and verified.

## Initial Scope

- Expose an LSP server over stdio.
- Provide `textDocument/codeAction` for call expressions under the cursor.
- Resolve safe, unambiguous PHP functions, methods, static methods, and
  constructors.
- Return `WorkspaceEdit` changes that insert `parameter_name: ` before
  positional arguments.
- Skip cases where conversion could change behavior or where symbol resolution
  is ambiguous.

## Non-goals

- Replacing Intelephense, Phpactor, or PHP Tools.
- Providing diagnostics, completion, hover, formatting, or navigation.
- Converting dynamic calls before semantic resolution is robust.
- Guessing parameter names from text when the callable cannot be resolved.

## Development

```sh
cargo fmt --check
cargo check
cargo test
```

## Install

Install the language server binary into Cargo's global bin directory:

```sh
cargo install --path .
which rephactor
```

Zed must be able to find `rephactor` on `PATH`. If `which rephactor` does not
return a path, add Cargo's bin directory to your shell profile:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

## Zed Setup

Install the local extension from `zed-extension/` with Zed's
`zed: install dev extension` command.

Keep a full PHP language server enabled for normal language intelligence and
run Rephactor alongside it:

```json
{
  "languages": {
    "PHP": {
      "language_servers": ["intelephense", "rephactor", "..."]
    }
  }
}
```

Rephactor currently provides only one refactor code action:
`Add names to arguments`.

## Supported Cases

- Same-file functions.
- Namespaced same-file functions.
- Static methods and constructors when the class is indexed.
- Instance methods when the receiver type is locally obvious from a typed
  parameter or `$var = new ClassName(...)`.
- Project symbols under Composer `autoload.psr-4` roots.

## Unsupported Cases

Rephactor returns no action instead of guessing for:

- dynamic calls such as `$fn(...)` or `$object->$method(...)`
- calls with unpacking (`...$args`)
- calls with existing named arguments
- ambiguous symbols
- unknown parameter names
- PHP internal functions or Composer classmaps
