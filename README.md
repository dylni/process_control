# Process Control

This crate allows running a process with resource limits, such as a running
time, and the option to terminate it automatically afterward. The latter is
surprisingly difficult to achieve on Unix, since process identifiers can be
arbitrarily reassigned when no longer used. Thus, it would be extremely easy to
inadvertently terminate an unexpected process. This crate protects against that
possibility.

Methods for setting limits are available on [`ChildExt`], which is implemented
for [`Child`]. They each return a builder of options to configure how the limit
should be applied.

***Warning**: This crate should not be used for security. There are many ways
that a process can bypass resource limits. The limits are only intended for
simple restriction of harmless processes.*

[![GitHub Build Status](https://github.com/dylni/process_control/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/dylni/process_control/actions/workflows/build.yml?query=branch%3Amaster)

## Usage

Add the following lines to your "Cargo.toml" file:

```toml
[dependencies]
process_control = "5.0"
```

See the [documentation] for available functionality and examples.

## Rust version support

The minimum supported Rust toolchain version is currently Rust 1.75.0.

Minor version updates may increase this version requirement. However, the
previous two Rust releases will always be supported. If the minimum Rust
version must not be increased, use a tilde requirement to prevent updating this
crate's minor version:

```toml
[dependencies]
process_control = "~5.0"
```

## License

Licensing terms are specified in [COPYRIGHT].

Unless you explicitly state otherwise, any contribution submitted for inclusion
in this crate, as defined in [LICENSE-APACHE], shall be licensed according to
[COPYRIGHT], without any additional terms or conditions.

### Third-party content

This crate includes copies and modifications of content developed by third
parties:

- [src/unix/read.rs] and [src/windows/read.rs] contain modifications of code
  from The Rust Programming Language, licensed under the MIT License or the
  Apache License, Version 2.0.

See those files for more details.

Copies of third-party licenses can be found in [LICENSE-THIRD-PARTY].

[`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
[`ChildExt`]: https://docs.rs/process_control/*/process_control/trait.ChildExt.html
[COPYRIGHT]: https://github.com/dylni/process_control/blob/master/COPYRIGHT
[documentation]: https://docs.rs/process_control
[LICENSE-APACHE]: https://github.com/dylni/process_control/blob/master/LICENSE-APACHE
[LICENSE-THIRD-PARTY]: https://github.com/dylni/process_control/blob/master/LICENSE-THIRD-PARTY
[src/unix/read.rs]: https://github.com/dylni/process_control/blob/master/src/unix/read.rs
[src/windows/read.rs]: https://github.com/dylni/process_control/blob/master/src/windows/read.rs
