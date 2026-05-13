# Rephactor

Rephactor is a PHP language server focused on semantic refactoring and
project-aware editor intelligence. It started as a companion server for a narrow
PHP 8 named-arguments refactor and is evolving toward a primary PHP language
server alternative.

The intended editor setup is to keep a full PHP language server such as
Intelephense or Phpactor for completion, diagnostics, hover, and navigation,
then run this server alongside it for a narrow code action:

```php
dispatch($order, true);
```

```php
dispatch(order: $order, notify: true);
```

This project grows in small verified slices. The named-arguments workflow
remains the correctness baseline while primary-LSP capabilities are added on top
of the same resolver and project index.

## Initial Scope

- Expose an LSP server over stdio.
- Provide `textDocument/codeAction` for call expressions under the cursor.
- Resolve safe, unambiguous PHP functions, methods, static methods, and
  constructors.
- Return `WorkspaceEdit` changes that insert `parameter_name: ` before
  positional arguments.
- Provide conservative class import refactor code actions.
- Provide `textDocument/signatureHelp` for resolved callables.
- Provide `textDocument/definition` for resolved project symbols.
- Provide `textDocument/hover` for resolved symbols.
- Provide deterministic `textDocument/completion` for basic symbols.
- Provide `textDocument/documentSymbol` for outline and breadcrumbs.
- Provide `workspace/symbol` for Composer-indexed project symbols.
- Provide `textDocument/references` for exact AST symbol references.
- Publish parse diagnostics for open PHP documents.
- Provide `textDocument/documentHighlight` for same-file symbol highlights.
- Skip cases where conversion could change behavior or where symbol resolution
  is ambiguous.

## Non-goals

- Providing diagnostics, completion, hover, or formatting.
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

Tagged releases publish prebuilt macOS, Linux, and Windows binaries through the
release workflow. During active development, `cargo install --path .` remains
the fastest local install path.

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

Rephactor currently provides named-argument and class-import refactor code
actions, Signature Help V1, Go To Definition V1, Hover V1, Completion V1, and
Document Symbol V1, Workspace Symbol V1, References V1, Diagnostics V1, and
Document Highlight V1.
The code action is titled `[Rephactor] Add names to arguments` when multiple
identifiers can be inserted. When only one positional argument is missing a
name, the title names that identifier, for example
`[Rephactor] Add name identifier 'exchange_gift'`.

Signature Help V1 shows parameter names for resolved functions, static methods,
constructors, and locally obvious instance methods. It intentionally returns no
signature for unsupported or ambiguous calls instead of guessing.

Go To Definition V1 navigates to resolved functions, classes, methods, static
methods, constructors, traits, interfaces, and imports that are present in the
project index. It returns no location for dynamic or ambiguous symbols.

Hover V1 shows a concise PHP signature or class FQN, source location, and the
nearest PHPDoc summary when available. It intentionally avoids full PHPDoc
rendering and returns no hover for ambiguous or dynamic symbols.

Completion V1 returns deterministic prefix matches for indexed class names,
indexed project functions, seeded PHP internal functions, static methods after
`ClassName::`, and instance methods when the receiver type is locally obvious.
It intentionally avoids snippets and fuzzy ranking.

Document Symbol V1 returns functions, classes, interfaces, traits, and class
methods for editor outline and breadcrumb UIs.

Workspace Symbol V1 searches Composer-indexed functions, classes, and methods
with deterministic case-insensitive matching.

References V1 finds exact matching AST name references across Composer-indexed
PHP files and open document overlays. It is intentionally conservative and does
not yet perform full type-aware disambiguation.

Diagnostics V1 publishes parser error diagnostics for open documents. Static
analysis diagnostics are still deferred until the type model is stronger.

Document Highlight V1 highlights exact matching AST names in the current
document.

Import refactors support adding an import for a resolvable fully-qualified
class name, shortening that usage, sorting simple class imports, and removing
unused simple class imports. Function imports, const imports, and destructive
grouped-import rewrites are intentionally skipped.

Zed currently applies LSP code actions directly. It does not show a PHPStorm-
style diff preview when moving through code action menu items; the aside popover
only expands long action titles.

When Rephactor returns no action, it logs the reason through the LSP client.
In Zed logs, look for lines like:

```text
Rephactor codeAction file:///path/File.php:2739:28 -> 0 action(s) in 4ms (index cache hit /path): unresolved callable customer_supplier::accumulatePoints
```

Project symbols are cached per Composer root after the first request. Open PHP
documents still override cached disk symbols. Disk changes in files that are
not open in the editor are picked up after restarting the language server; a
file watcher is intentionally deferred.

## Supported Cases

- Same-file functions.
- Basic class, function, static method, and locally obvious instance method
  completions.
- Document symbols for functions, class-like declarations, and methods.
- Workspace symbols for indexed functions, classes, and methods.
- Exact AST references across Composer-indexed PHP files.
- Parse diagnostics for open PHP documents.
- Same-file document highlights for exact AST name matches.
- Conservative class import refactors for normal `use Foo\Bar;` declarations.
- Namespaced same-file functions.
- Static methods and constructors when the class is indexed, including class
  names imported with normal, grouped, or aliased `use` declarations.
- Instance methods when the receiver type is locally obvious from a typed
  parameter or `$var = new ClassName(...)`.
- Project symbols under Composer `autoload.psr-4` roots.
- Project symbols under Composer `autoload.classmap` files or directories.
- A small seed set of PHP internal functions, such as `str_replace`,
  `json_encode`, `preg_match`, and `in_array`.
- Calls that already contain safe named arguments and still have remaining
  positional arguments. Rephactor inserts only the missing names.
- Projects without a Composer PHP version constraint, or projects whose
  `require.php` constraint requires PHP 8 or newer.

## Unsupported Cases

Rephactor returns no action instead of guessing for:

- dynamic calls such as `$fn(...)` or `$object->$method(...)`
- calls with unpacking (`...$args`)
- function imports, const imports, and grouped-import rewrites
- calls whose existing named arguments do not match the resolved signature
- ambiguous symbols
- unknown parameter names
- PHP internal functions outside the seeded stub set
- completion for dynamic receivers or unresolved classes
- static-analysis diagnostics beyond parser errors
- Composer autoload modes other than `autoload.psr-4` and `autoload.classmap`
- parent/interface/trait resolution that depends on unindexed or ambiguous
  symbols
- PHPStan/Psalm metadata, including generics, template annotations, and
  framework-specific dynamic return type extensions
- Composer `require.php` constraints that allow PHP 7.x, because named
  arguments require PHP 8

## Release Status

Current release posture:

- Build from source with `cargo install --path .`.
- Install the local Zed extension from `zed-extension/`.
- Verify the Rust server with `cargo fmt --check`, `cargo check`,
  `cargo test`, and `cargo clippy -- -D warnings`.
- Verify the Zed extension with `scripts/check-zed-extension.sh`.

Release binaries are built by GitHub Actions when a `v*` tag is pushed.

Deferred until V1 behavior is stable:

- full PHP internal stubs
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
7. Put the cursor inside an unsupported call and verify that Zed logs include a
   concise no-action reason.
8. Trigger signature help inside a supported call and verify that the active
   parameter follows the cursor.
