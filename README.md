Mavio
=====

Minimalistic library for transport-agnostic [MAVLink](https://mavlink.io/en/) communication. It supports `no-std`
(and `no-alloc`) targets.

[`repository`](https://gitlab.com/mavka/libs/mavio)
[`crates.io`](https://crates.io/crates/mavio)
[`API docs`](https://docs.rs/mavio/latest/mavio/)
[`issues`](https://gitlab.com/mavka/libs/mavio/-/issues)

Mavio is a building block for more sophisticated tools. It is entirely focused on one thing: to include absolute minimum
of functionality required for correct communication with everything that speaks MAVLink protocol.

* It supports both `MAVLink 1` and `MAVLink 2` protocol versions.
* Provides intermediate MAVLink packets decoding as "frames" that contain only header, checksum and signature being
  deserialized. Which means that client don't have to decode the entire message for routing and verification.
* Supports optional high-level message decoding by utilizing MAVLink abstractions generated by
  [MAVSpec](https://gitlab.com/mavka/libs/mavspec).
* Includes standard MAVLink dialects enabled by Cargo features.
* Implements message verification via checksum.
* Includes tools for [message signing](https://mavlink.io/en/guide/message_signing.html).

In other words, Mavio implements all *stateless* features of MAVLink protocol. Which means that it does not provide
support for message sequencing, automatic heartbeats, etc. The client is responsible for implementing these parts of the
protocol by their own or use a dedicated library. We've decided to keep Mavio as simple and catchy as an
[8-bit melody](https://archive.org/details/SuperMarioBros.ThemeMusic).

At the same time, Mavio is flexible and tries to dictate as few as possible. In particular:

* It supports custom dialects or may work with no dialect at all (for intermediate decoding). This is useful if you want
  to simply route or sign messages. 
* Includes support for custom payload decoders and encoders. Which means that clients are not bounded by
  abstractions provided by build-in code generator.
* Can read and write messages to anything that implements [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html)
  and [`std::io::Write`](https://doc.rust-lang.org/std/io/trait.Write.html) traits.
* Compatible with `no_std` targets. For such cases the library provides simplified versions of `Read` and `Write`
  traits.

This library is a part of [Mavka](https://mavka.gitlab.io/home/) toolchain. It is integrated with other projects such
as:

* [MAVInspect](https://gitlab.com/mavka/libs/mavinspect) that responsible for MAVLink XML definitions parsing.
* [MAVSpec](https://gitlab.com/mavka/libs/mavinspect) that focused on code generation. Mavio uses this library to
  generate MAVLink dialects.
* [Maviola](https://gitlab.com/mavka/libs/maviola) (WIP), an elaborated MAVLink communication library that takes care about **stateful** features:
* sequencing, message time-stamping, automatic heartbeats, simplifies message signing, and so on. Mavio serves as a
  basis for Maviola.

This project respects [`semantic versioning`](https://semver.org).

Install
-------

Install Mavio with cargo:

```shell
cargo add mavio
```

Usage
-----

License
-------

> Here we simply comply with the suggested dual licensing according to
> [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/about.html) (C-PERMISSIVE).

Licensed under either of

* Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license
  ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Contribution
------------

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
