Mavio
=====

Minimalistic library for transport-agnostic [MAVLink](https://mavlink.io/en/) communication. It supports `no-std`
(and `no-alloc`) targets.

<span style="font-size:24px">[🇺🇦](https://mavka.gitlab.io/home/a_note_on_the_war_in_ukraine/)</span>
[![`repository`](https://img.shields.io/gitlab/pipeline-status/mavka/libs/mavio.svg?branch=main&label=repository)](https://gitlab.com/mavka/libs/mavio)
[![`crates.io`](https://img.shields.io/crates/v/mavio.svg)](https://crates.io/crates/mavio)
[![`docs.rs`](https://img.shields.io/docsrs/mavio.svg?label=docs.rs)](https://docs.rs/mavio/latest/mavio/)
[![`issues`](https://img.shields.io/gitlab/issues/open/mavka/libs/mavio.svg)](https://gitlab.com/mavka/libs/mavio/-/issues/)

<details>
<summary>
More on MAVLink
</summary>

MAVLink is a lightweight open protocol for communicating between drones, onboard components and ground control stations.
It is used by such autopilots like [PX4](https://px4.io) or [ArduPilot](https://ardupilot.org/#). MAVLink has simple and
compact serialization model. The basic abstraction is `message` which can be sent through a link (UDP, TCP, UNIX
socket, UART, whatever) and deserialized into a struct with fields of primitive types or arrays of primitive types.
Such fields can be additionally restricted by `enum` variants, annotated with metadata like units of measurements,
default or invalid values.

There are several MAVLink dialects. Official dialect definitions are
[XML files](https://mavlink.io/en/guide/xml_schema.html) that can be found in the MAVlink
[repository](https://github.com/mavlink/mavlink/tree/master/message_definitions/v1.0). Based on `message` abstractions,
MAVLink defines so-called [`microservices`](https://mavlink.io/en/services/) that specify how clients should respond on
a particular message under certain conditions or how they should initiate a particular action.
</details>

Mavio is a building block for more sophisticated tools. It is entirely focused on one thing: to include absolute minimum
of functionality required for correct communication with everything that speaks MAVLink protocol.

* Supports both `MAVLink 1` and `MAVLink 2` protocol versions.
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

* It supports [custom dialects](#custom-dialects) or may work with no dialect at all (for intermediate decoding). The
  latter is useful if you want to simply route or sign messages.
* Can read and write messages to anything that implements [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html)
  and [`std::io::Write`](https://doc.rust-lang.org/std/io/trait.Write.html) traits.
* Compatible with `no_std` targets. For such cases the library provides simplified versions of `Read` and `Write`
  traits.
* Support asynchronous I/O via [Tokio](https://tokio.rs).
* Allows filtering out unnecessary MAVLink entities (i.e. messages, enums, commands) to reduce compilation time.

This library is a part of [Mavka](https://mavka.gitlab.io/home/) toolchain. It is integrated with other projects such
as:

* [MAVInspect](https://gitlab.com/mavka/libs/mavinspect) that responsible for MAVLink XML definitions parsing.
* [MAVSpec](https://gitlab.com/mavka/libs/mavinspect) that focused on code generation. Mavio uses this library to
  generate MAVLink dialects.
* [Maviola](https://gitlab.com/mavka/libs/maviola) (WIP), is a MAVLink communication library based on `Mavio` that
  provides a high-level interface for MAVLink messaging and takes care about **stateful** features of the protocol:
  sequencing, message time-stamping, automatic heartbeats, simplifies message signing, and so on.

This project respects [`semantic versioning`](https://semver.org). As allowed by specification, breaking changes may be
introduced in minor releases until version `1.0.0` is reached. However, we will keep unstable features under the
`unstable` feature flag whenever possible.

Install
-------

Install Mavio with cargo:

```shell
cargo add mavio
```

Usage
-----

For details, please check [API](#api-notes) section and [API documentation](https://docs.rs/mavio/latest/mavio/).

### Receiving MAVLink Frames

Connect as TCP client, receive 10 frames, and decode any received
[`HEARTBEAT`](https://mavlink.io/en/messages/common.html#HEARTBEAT) message:

```rust
use std::net::TcpStream;
use mavio::{Frame, Receiver};
use mavio::dialects::minimal as dialect;
use dialect::Message;

fn main() -> mavio::errors::Result<()> {
    let mut receiver = Receiver::new(TcpStream::connect("0.0.0.0:5600")?);

    for i in 0..10 {
        let frame = receiver.recv()?;

        if let Err(err) = frame.validate_checksum(dialect::spec()) {
            eprintln!("Invalid checksum: {:?}", err);
            continue;
        }

        if let Ok(Message::Heartbeat(msg)) = dialect::decode(frame.payload()) {
            println!(
                "HEARTBEAT #{}: mavlink_version={:#?}",
                frame.sequence(),
                msg.mavlink_version,
            );
        }
    }
}
```

A slightly more elaborated use-case can be found in [`tcp_client.rs`](./examples/sync/examples/tcp_client.rs) example.

### Sending MAVLink Frames

Listen to TCP port as a server, send 10 [`HEARTBEAT`](https://mavlink.io/en/messages/common.html#HEARTBEAT) messages
to any connected client using MAVLink 2 protocol, then disconnect a client.

```rust
use std::net::TcpStream;
use mavio::{Frame, Sender};
use mavio::protocol::MavLinkVersion;
use mavio::dialects::minimal as dialect;
use dialect::Message;
use dialect::enums::{MavAutopilot, MavModeFlag, MavState, MavType};

fn main() -> mavio::errors::Result<()> {
    let mut sender = Sender::new(TcpStream::connect("0.0.0.0:5600")?);

    let mavlink_version = MavLinkVersion::V2;
    let system_id = 15;
    let component_id = 42;

    for sequence in 0..10 {
        let message = dialect::messages::Heartbeat {
            type_: MavType::FixedWing,
            autopilot: MavAutopilot::Generic,
            base_mode: MavModeFlag::TEST_ENABLED
                & MavModeFlag::CUSTOM_MODE_ENABLED,
            custom_mode: 0,
            system_status: MavState::Active,
            mavlink_version: 3,
        };
        println!("MESSAGE #{}: {:#?}", sequence, message);

        let frame = Frame::builder()
            .set_sequence(sequence)
            .set_system_id(system_id)
            .set_component_id(component_id)
            .build_for(&message, mavlink_version)?;

        sender.send(&frame)?;
        println!("FRAME #{} sent: {:#?}", sequence, frame);
    }
}
```

Check [`tcp_server.rs`](./examples/sync/examples/tcp_server.rs) for a slightly more elaborated use-case.

API Notes
---------

This section provides a general API overview. For further details, please check
[API documentation](https://docs.rs/mavio/latest/mavio/).

### I/O

Mavio provides two basic I/O primitives: [`Sender`](https://docs.rs/mavio/latest/mavio/struct.Sender.html) and
[`Receiver`](https://docs.rs/mavio/latest/mavio/struct.Sender.html). These structs send and receive instances of
[`Frame`](https://docs.rs/mavio/latest/mavio/struct.Frame.html).

`Sender` and `Receiver` are generic over [`std::io::Write`](https://doc.rust-lang.org/std/io/trait.Write.html) and
[`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html) accordingly. That means you can communicate MAVLink
messages over various transports which implements these traits including UDP, TCP, Unix sockets, and files. It is also
easy to implement custom transport.

For `no-std` targets Mavio provides custom implementations of `Read` and `Write` traits. You can implement them for
hardware-specific communication (like serial ports).

For asynchronous I/O Mavio provides [`AsyncSender`](https://docs.rs/mavio/latest/mavio/struct.AsyncSender.html) and
[`AsyncReceiver`](https://docs.rs/mavio/latest/mavio/struct.AsyncReceiver.html) which work in the same fashion as their
synchronous counterparts, except using [`Tokio`](https://tokio.rs). To enable asynchronous support, add `tokio` feature
flag.

### Encoding/Decoding

Upon receiving, MAVLink [`Frame`](https://docs.rs/mavio/latest/mavio/struct.Frame.html)s can be validated and decoded
into MAVLink messages. Frames can be routed, signed, or forwarded to another system/component ID without decoding.

> **Note!**
> 
> MAVLink checksum validation requires [`CRC_EXTRA`](https://mavlink.io/en/guide/serialization.html#crc_extra)
> byte which its turn depends on a dialect specification. That means, if you are performing dialect-agnostic routing
> from a noisy source or from devices which implement outdated message specifications, you may forward junk messages.
> In case of high-latency channels you might want to enforce compliance with a particular dialect to filter
> incompatible messages.

To decode a frame into a MAVLink message, you need to use a specific dialect. Standard MAVLink dialects are available
under [`mavio::dialects`](https://docs.rs/mavio/latest/mavio/dialects) and can be enabled by the corresponding
feature flags:

* [`minimal`]((https://mavlink.io/en/messages/minimal.html)) — minimal dialect required to expose your presence to
  other MAVLink devices.
* [`standard`](https://mavlink.io/en/messages/standard.html) — a superset of `minimal` dialect which expected to be
  used by almost all flight stack.
* [`common`](https://mavlink.io/en/messages/common.html) — minimum viable dialect with most of the features, a
  building block for other future-rich dialects.
* [`ardupilotmega`](https://mavlink.io/en/messages/common.html) — feature-full dialect used by
  [ArduPilot](http://ardupilot.org). In most cases this dialect is the go-to choice if you want to recognize almost
  all MAVLink messages used by existing flight stacks.
* [`all`](https://mavlink.io/en/messages/all.html) — meta-dialect which includes all other standard dialects
  including these which were created for testing purposes. It is guaranteed that namespaces of the dialects in `all`
  family do not collide.
* Other dialects from MAVLink XML [definitions](https://github.com/mavlink/mavlink/tree/master/message_definitions/v1.0):
  `asluav`, `avssuas`, `csairlink`, `cubepilot`, `development`, `icarous`, `matrixpilot`, `paparazzi`, `ualberta`,
  `uavionix`. These do not include `python_array_test` and `test` dialects which should be either generated manually
  or as a part of `all` meta-dialect.

### Custom Dialects

Concrete implementations of dialects are generated by [MAVSpec](https://gitlab.com/mavka/libs/mavspec) check its
[documentation](https://docs.rs/mavspec/latest/mavspec/) for details. You may also found useful to review
[`build.rs`](mavio/build.rs), if you want to generate your custom dialects.

Examples
--------

Examples for synchronous I/O in [`./examples/sync/examples`](./examples/sync/examples):

* [`tcp_server.rs`](./examples/sync/examples/tcp_server.rs) is a simple TCP server that awaits for connections, sends
  and receives heartbeats:
  ```shell
  cargo run --package mavio_examples_sync --example tcp_server
  ```
* [`tcp_client.rs`](./examples/sync/examples/tcp_client.rs) is a TCP client which connects to server, sends and receives
  heartbeats:
  ```shell
  cargo run --package mavio_examples_sync --example tcp_client
  ```
* [`tcp_ping_pong.rs`](./examples/sync/examples/tcp_ping_pong.rs) server and clients which communicate with each other
  via TCP:
  ```shell
  cargo run --package mavio_examples_sync --example tcp_ping_pong
  ```

Examples for asynchronous I/O in [`./examples/async/examples`](./examples/async/examples):

* [`async_tcp_ping_pong.rs`](./examples/async/examples/async_tcp_ping_pong.rs) server and clients which communicate with
  each other via TCP:
  ```shell
  cargo run --package mavio_examples_async --example async_tcp_ping_pong
  ```

Examples for custom dialect generation with filtered MAVLink entities can be found in
[`./examples/custom/examples`](examples/custom/examples):

* [`custom_dialects_usage.rs`](examples/custom/examples/custom_dialects_usage.rs) a basic usage of
  custom-generated dialect:
  ```shell
  cargo run --package mavio_examples_custom --example mavio_examples_custom_usage
  ```
* [`custom_message.rs`](examples/custom/examples/custom_message.rs) crating and using a custom message:
  ```shell
  cargo run --package mavio_examples_custom --example custom_message
  ```

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
