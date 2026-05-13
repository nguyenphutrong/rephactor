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

Prebuilt release binaries are not published yet. Until the V1 behavior settles,
the supported install path is `cargo install --path .` from this repository.

## Zed Setup

Zed compiles dev extensions to Wasm. Make sure Zed sees the Rustup toolchain
before Homebrew Rust, then install the Wasm target:

```sh
rustup target add wasm32-wasip2
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --manifest-path zed-extension/Cargo.toml --target wasm32-wasip2
```

The repository also includes the same check as a script:

```sh
scripts/check-zed-extension.sh
```

If `which cargo` prints `/opt/homebrew/bin/cargo`, move `$HOME/.cargo/bin`
earlier in your shell `PATH`. Otherwise Zed may fail to install the dev
extension with `can't find crate for core` for `wasm32-wasip2`.

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

Rephactor currently provides one narrow refactor code action. It is titled
`Add names to arguments` when multiple identifiers can be inserted. When only
one positional argument is missing a name, the title names that identifier, for
example `Add name identifier 'exchange_gift'`.

Zed currently applies LSP code actions directly. It does not show a PHPStorm-
style diff preview when moving through code action menu items; the aside popover
only expands long action titles.

## Supported Cases

- Same-file functions.
- Namespaced same-file functions.
- Static methods and constructors when the class is indexed, including class
  names imported with normal, grouped, or aliased `use` declarations.
- Instance methods when the receiver type is locally obvious from a typed
  parameter or `$var = new ClassName(...)`.
- Project symbols under Composer `autoload.psr-4` roots.
- Project symbols under Composer `autoload.classmap` files or directories.
- Calls that already contain safe named arguments and still have remaining
  positional arguments. Rephactor inserts only the missing names.
- Projects without a Composer PHP version constraint, or projects whose
  `require.php` constraint requires PHP 8 or newer.

## Unsupported Cases

Rephactor returns no action instead of guessing for:

- dynamic calls such as `$fn(...)` or `$object->$method(...)`
- calls with unpacking (`...$args`)
- calls whose existing named arguments do not match the resolved signature
- ambiguous symbols
- unknown parameter names
- PHP internal functions
- Composer autoload modes other than `autoload.psr-4` and `autoload.classmap`
- parent/interface/trait resolution that depends on unindexed or ambiguous
  symbols
- Composer `require.php` constraints that allow PHP 7.x, because named
  arguments require PHP 8

## Release Status

Current release posture:

- Build from source with `cargo install --path .`.
- Install the local Zed extension from `zed-extension/`.
- Verify the Rust server with `cargo fmt --check`, `cargo check`,
  `cargo test`, and `cargo clippy -- -D warnings`.
- Verify the Zed extension with `scripts/check-zed-extension.sh`.

Deferred until V1 behavior is stable:

- prebuilt macOS, Linux, and Windows binaries
- PHP internal stubs
- PHPStan/Psalm metadata

## Manual Acceptance

1. Install the binary with `cargo install --path .`.
2. Install the local Zed extension from `zed-extension/`.
3. Open a PHP project that has Composer PSR-4 autoloading.
4. Keep Intelephense or Phpactor enabled and add `rephactor` to the PHP
   `language_servers` list.
5. Put the cursor inside a supported call expression and run code actions.
6. Apply the Rephactor code action and verify that only `parameter_name: `
   prefixes were inserted.
