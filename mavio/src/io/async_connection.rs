//! # MAVLink connection

use std::{io, net::ToSocketAddrs};

use tokio::{
    io::{split, AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream, UdpSocket},
};
use tokio_serial::SerialPortBuilderExt;
use udp_stream::{UdpListener, UdpStream};

use crate::{prelude::MaybeVersioned, protocol::Versioned, protocol::Versionless, Frame, Result};

use super::{AsyncReceiver, AsyncSender};

/// An async MAVLink connection.
///
/// Wraps an [`AsyncReceiver`] and [`AsyncSender`] internally, with the same MAVLink version [`V`].
/// This can be useful when you dont want to create separate objects and handle with stream splitting.
pub struct AsyncConnection<V: MaybeVersioned> {
    receiver: AsyncReceiver<Box<dyn AsyncRead + Send + Unpin>, V>,
    sender: AsyncSender<Box<dyn AsyncWrite + Send + Unpin>, V>,
    // TODO: add protocol_version to change it at runtime?
}

impl<V: MaybeVersioned> AsyncConnection<V> {
    /// Receives MAVLink [`Frame`].
    ///
    /// Blocks until a valid MAVLink frame received.
    ///
    /// [`Versioned`] connection accepts only frames of a specific MAVLink protocol version.
    ///
    /// [`Versionless`] connection accepts both `MAVLink 1` and `MAVLink 2` frames.
    pub async fn recv(&mut self) -> Result<Frame<V>> {
        self.receiver.recv().await
    }

    /// Send MAVLink [`Frame`] asynchronously.
    ///
    /// [`Versioned`] connection accepts only frames of a specific MAVLink protocol version.
    ///
    /// [`Versionless`] connection accepts both `MAVLink 1` and `MAVLink 2` frames as
    /// [`Frame<Versionless>`].
    ///
    /// Returns the number of bytes sent.
    pub async fn send(&mut self, frame: &Frame<V>) -> Result<usize> {
        self.sender.send(frame).await
    }
}

/// Connect asynchronously to a MAVLink node by address string by returning an [`AsyncConnection`].
///
/// The address must be in one of the following formats:
///
///  * `tcpin:<addr>:<port>` to create a TCP server, listening for incoming connections
///  * `tcpout:<addr>:<port>` to create a TCP client
///  * `udpin:<addr>:<port>` to create a UDP server, listening for incoming packets
///  * `udpout:<addr>:<port>` to create a UDP client
///  * `udpbcast:<addr>:<port>` to create a UDP broadcast
///  * `file:<path>` to extract file data
///  * `serial:<device>:<baudrate>` to create a serial connection.
pub async fn connect_async<V>(address: &str) -> io::Result<AsyncConnection<V>>
where
    V: MaybeVersioned,
{
    let mut parts = address.split(':');
    let protocol = parts
        .next()
        .expect("Invalid protocol: <protocol>:<address/device>:<port/baud>");

    match protocol {
        "tcpout" | "tcpin" => {
            let address = parts.collect::<Vec<&str>>().join(":");
            let stream = match protocol {
                "tcpout" => TcpStream::connect(address).await?,
                "tcpin" => TcpListener::bind(address).await?.accept().await?.0,
                _ => unreachable!(),
            };
            let (r, w) = split(stream);
            let receiver = AsyncReceiver::new::<V>(Box::new(r) as _);
            let sender = AsyncSender::new::<V>(Box::new(w) as _);
            Ok(AsyncConnection { receiver, sender })
        }
        "udpin" | "udpout" | "udpbcast" => {
            let address = parts.collect::<Vec<&str>>().join(":");
            let addr = address
                .to_socket_addrs()?
                .next()
                .expect("Invalid UDP address");
            let (reader, writer): (
                Box<dyn AsyncRead + Send + Unpin>,
                Box<dyn AsyncWrite + Send + Unpin>,
            ) = match protocol {
                "udpin" => {
                    let stream = UdpListener::bind(addr).await?.accept().await?.0;
                    let (r, w) = split(stream);
                    (Box::new(r) as _, Box::new(w) as _)
                }
                "udpout" => {
                    let stream = UdpStream::connect(addr).await?;
                    let (r, w) = split(stream);
                    (Box::new(r) as _, Box::new(w) as _)
                }
                "udpbcast" => {
                    let socket = UdpSocket::bind("0.0.0.0:0").await?;
                    socket.set_broadcast(true)?;
                    let stream = UdpStream::from_tokio(socket, addr).await?;
                    let (r, w) = split(stream);
                    (Box::new(r) as _, Box::new(w) as _)
                }
                _ => unreachable!(),
            };
            let receiver = AsyncReceiver::new::<V>(reader);
            let sender = AsyncSender::new::<V>(writer);
            Ok(AsyncConnection { receiver, sender })
        }
        "serial" => {
            let device = parts.next().ok_or(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unspecified serial port device: serial:<device>:<baud>",
            ))?;
            let baud_rate = parts
                .next()
                .ok_or(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unspecified baud rate: serial:<device>:<baud>",
                ))?
                .parse::<u32>()
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Invalid specified baud rate")
                })?;
            let stream = tokio_serial::new(device, baud_rate).open_native_async()?;
            let (r, w) = split(stream);
            let receiver = AsyncReceiver::new::<V>(Box::new(r) as _);
            let sender = AsyncSender::new::<V>(Box::new(w) as _);
            Ok(AsyncConnection { receiver, sender })
        }
        _ => Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "Protocol unsupported",
        )),
    }
}
