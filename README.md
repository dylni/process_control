# Process Control

This crate allows running a process with a timeout, with the option to
terminate it automatically afterward. The latter is surprisingly difficult to
achieve on Unix, since process identifiers can be arbitrarily reassigned when
no longer used. Thus, it would be extremely easy to inadvertently terminate an
unexpected process. This crate protects against that possibility.

Methods for creating timeouts are available on [`ChildExt`], which is
implemented for [`Child`]. They each return a builder of options to configure
how the timeout should be applied.

[![GitHub Build Status](https://github.com/dylni/process_control/workflows/build/badge.svg?branch=master)](https://github.com/dylni/process_control/actions?query=branch%3Amaster)

## Usage

Add the following lines to your "Cargo.toml" file:

```toml
[dependencies]
process_control = "0.7"
```

See the [documentation] for available functionality and examples.

## Rust version support

The minimum supported Rust toolchain version is currently Rust 1.36.0.

## License

Licensing terms are specified in [COPYRIGHT].

Unless you explicitly state otherwise, any contribution submitted for inclusion
in this crate, as defined in [LICENSE-APACHE], shall be licensed according to
[COPYRIGHT], without any additional terms or conditions.

[`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
[`ChildExt`]: https://docs.rs/process_control/*/process_control/trait.ChildExt.html
[COPYRIGHT]: https://github.com/dylni/process_control/blob/master/COPYRIGHT
[documentation]: https://docs.rs/process_control
[LICENSE-APACHE]: https://github.com/dylni/process_control/blob/master/LICENSE-APACHE
