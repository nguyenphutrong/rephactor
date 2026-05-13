# Semantic Metadata

Rephactor does not consume PHPStan or Psalm metadata in V1.

The current resolver only uses facts it can read directly from PHP source:

- same-file functions
- Composer PSR-4 and classmap project symbols
- class methods, static methods, constructors, traits, interfaces, and parent
  classes that are indexed without ambiguity
- locally obvious receiver types from typed parameters or `new ClassName()`

This keeps the code action conservative. If a callable or receiver type depends
on PHPStan/Psalm generics, template annotations, array-shape metadata, dynamic
return type extensions, or framework-specific plugins, Rephactor returns no
action instead of guessing.

Future PHPStan/Psalm support should be added only after the metadata source is
explicit and fixture-tested. The first useful slice would be read-only support
for simple `@var ClassName $variable` annotations near a call site, followed by
tests that prove unsupported generic or ambiguous annotations still return no
action.
