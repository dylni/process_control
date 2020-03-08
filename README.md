# Process Control

This crate allows terminating a process without a mutable reference.
[`ProcessTerminator::terminate`] is designed to operate in this manner and is
the reason this crate exists. It intentionally does not require a reference of
any kind to the [`Child`] instance, allowing for maximal flexibility in working
with processes.

Typically, it is not possible to terminate a process during a call to
[`Child::wait`] or [`Child::wait_with_output`] in another thread, since
[`Child::kill`] takes a mutable reference. However, since this crate creates
its own termination method, there is no issue, allowing system resources to be
freed when using methods such as [`ChildExt::with_output_timeout`].

[![GitHub Build Status](https://github.com/dylni/process_control/workflows/build/badge.svg?branch=master)](https://github.com/dylni/process_control/actions?query=branch%3Amaster)

## Usage

Add the following lines to your "Cargo.toml" file:

```toml
[dependencies]
process_control = "0.4"
```

See the [documentation] for available functionality and examples.

## Rust version support

The minimum supported Rust toolchain version is currently Rust 1.34.0.

## License

Licensing terms are specified in [COPYRIGHT].

Unless you explicitly state otherwise, any contribution submitted for inclusion
in this crate, as defined in [LICENSE-APACHE], shall be licensed according to
[COPYRIGHT], without any additional terms or conditions.

[`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
[`Child::kill`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.kill
[`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
[`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
[`ChildExt::with_output_timeout`]: https://docs.rs/process_control/*/process_control/trait.ChildExt.html#tymethod.with_output_timeout
[COPYRIGHT]: https://github.com/dylni/process_control/blob/master/COPYRIGHT
[documentation]: https://docs.rs/process_control
[LICENSE-APACHE]: https://github.com/dylni/process_control/blob/master/LICENSE-APACHE
[`ProcessTerminator::terminate`]: https://docs.rs/process_control/*/process_control/struct.ProcessTerminator.html#method.terminate
