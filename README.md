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
cargo check
cargo test
```

The first implementation milestone is a no-op LSP server that advertises code
actions and returns an empty action list. Semantic conversion comes after that
baseline is testable.
