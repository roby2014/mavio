//! # MAVLink header
//!
//! This module contains implementation for MAVLink packet header both for `MAVLink 1` and
//! `MAVLink 2` protocol versions.

use core::marker::PhantomData;
use tbytes::{TBytesReader, TBytesReaderFor};

#[cfg(feature = "tokio")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::consts::{
    CHECKSUM_SIZE, HEADER_MAX_SIZE, HEADER_MIN_SIZE, HEADER_V1_SIZE, HEADER_V2_SIZE,
    MAVLINK_IFLAG_SIGNED, SIGNATURE_LENGTH,
};
use crate::io::{Read, Write};
use crate::protocol::marker::{NoCompId, NoMsgId, NoPayloadLen, NoSysId, NotSequenced};
use crate::protocol::{
    CompatFlags, ComponentId, HeaderBuilder, IncompatFlags, MavSTX, MaybeVersioned, PayloadLength,
    Sequence, SystemId, Versioned, Versionless, V2,
};
use crate::protocol::{MavLinkVersion, MessageId};

use crate::prelude::*;

/// MAVLink frame header.
///
/// Header contains information relevant to for `MAVLink 1` and `MAVLink 2` packet formats.
///
/// # Versioned and versionless headers
///
/// In most cases, you are going to receive a [`Versionless`] header. However, if you want to work
/// in a context of a specific MAVLink protocol version, you can convert header into a [`Versioned`]
/// variant by calling [`Header::try_versioned`].
///
/// You always can forget about header's version by calling [`Header::versionless`].
///
/// # Links
///
///  * [MAVLink 1 packet format](https://mavlink.io/en/guide/serialization.html#v1_packet_format).
///  * [MAVLink 2 packet format](https://mavlink.io/en/guide/serialization.html#mavlink2_packet_format).
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Header<V: MaybeVersioned> {
    pub(super) mavlink_version: MavLinkVersion,
    pub(super) payload_length: PayloadLength,
    pub(super) incompat_flags: IncompatFlags,
    pub(super) compat_flags: CompatFlags,
    pub(super) sequence: Sequence,
    pub(super) system_id: SystemId,
    pub(super) component_id: ComponentId,
    pub(super) message_id: MessageId,
    pub(super) _marker_version: PhantomData<V>,
}

/// Represents [`Header`] encoded as a sequence of bytes.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeaderBytes {
    buffer: [u8; HEADER_MAX_SIZE],
    size: usize,
}

impl HeaderBytes {
    /// Encoded [`Header`] as a slice of bytes.
    ///
    /// The length of a slice matches [`Self::size`] and therefore [`Header::size`].
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[0..self.size]
    }

    /// Size of the encoded [`Header`] in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Encoded [`Header`] CRC data.
    ///
    /// Returns all header data excluding `magic` byte.
    ///
    /// See:
    ///  * [`MavLinkFrame::calculate_crc`](crate::protocol::Frame::calculate_crc).
    ///  * [MAVLink checksum](https://mavlink.io/en/guide/serialization.html#checksum) in MAVLink
    ///    protocol documentation.
    pub fn crc_data(&self) -> &[u8] {
        &self.buffer[1..self.size()]
    }
}

impl<V: MaybeVersioned> Header<V> {
    /// MAVLink protocol version.
    ///
    /// MAVLink version defined by the magic byte (STX).
    ///
    /// See [`MavSTX`].
    #[inline]
    pub fn mavlink_version(&self) -> MavLinkVersion {
        self.mavlink_version
    }

    /// Payload length.
    ///
    /// Indicates length of the following `payload` section. This may be affected by payload truncation.
    #[inline]
    pub fn payload_length(&self) -> PayloadLength {
        self.payload_length
    }

    /// Packet sequence number.
    ///
    /// Used to detect packet loss. Components increment value for each message sent.
    #[inline]
    pub fn sequence(&self) -> Sequence {
        self.sequence
    }

    /// System `ID`.
    ///
    /// `ID` of system (vehicle) sending the message. Used to differentiate systems on network.
    ///
    /// > Note that the broadcast address 0 may not be used in this field as it is an invalid source
    /// > address.
    #[inline]
    pub fn system_id(&self) -> SystemId {
        self.system_id
    }

    /// Component `ID`.
    ///
    /// `ID` of component sending the message. Used to differentiate components in a system (e.g.
    /// autopilot and a camera). Use appropriate values in
    /// [MAV_COMPONENT](https://mavlink.io/en/messages/common.html#MAV_COMPONENT).
    ///
    /// > Note that the broadcast address `MAV_COMP_ID_ALL` may not be used in this field as it is
    /// > an invalid source address.
    #[inline]
    pub fn component_id(&self) -> ComponentId {
        self.component_id
    }

    /// Message `ID`.
    ///
    /// `ID` of MAVLink message. Defines how payload will be encoded and decoded.
    #[inline]
    pub fn message_id(&self) -> MessageId {
        self.message_id
    }

    /// Size of the header in bytes.
    ///
    /// Depends on the MAVLink protocol version.
    pub fn size(&self) -> usize {
        match self.mavlink_version {
            MavLinkVersion::V1 => HEADER_V1_SIZE,
            MavLinkVersion::V2 => HEADER_V2_SIZE,
        }
    }

    /// Returns `true` if frame body should contain signature.
    ///
    /// For `MAVLink 1` headers always returns `false`.
    ///
    /// For `MAVLink 2` it checks for [`MAVLINK_IFLAG_SIGNED`] (default is `false`).
    /// returned.
    ///
    /// # Links
    ///
    /// * [Frame::signature](crate::protocol::Frame::signature).
    pub fn is_signed(&self) -> bool {
        match self.mavlink_version {
            MavLinkVersion::V1 => false,
            MavLinkVersion::V2 => {
                self.incompat_flags & MAVLINK_IFLAG_SIGNED == MAVLINK_IFLAG_SIGNED
            }
        }
    }

    /// Sets whether `MAVLink 2` frame body should contain signature.
    ///
    /// Sets `MAVLINK_IFLAG_SIGNED` for [`Self::incompat_flags`].
    #[inline]
    pub(super) fn set_is_signed(&mut self, flag: bool) {
        self.incompat_flags =
            self.incompat_flags & !MAVLINK_IFLAG_SIGNED | (MAVLINK_IFLAG_SIGNED & flag as u8);
    }

    /// MAVLink frame body length.
    ///
    /// Calculates expected size in bytes for frame body. Depends on MAVLink protocol version and presence of
    /// signature (when [`MAVLINK_IFLAG_SIGNED`] incompatibility flag is set).
    ///
    /// # Links
    /// * [`Frame::signature`](crate::protocol::Frame::signature).
    pub fn body_length(&self) -> usize {
        match self.mavlink_version {
            MavLinkVersion::V1 => self.payload_length as usize + CHECKSUM_SIZE,
            MavLinkVersion::V2 => {
                if self.is_signed() {
                    self.payload_length as usize + CHECKSUM_SIZE + SIGNATURE_LENGTH
                } else {
                    self.payload_length as usize + CHECKSUM_SIZE
                }
            }
        }
    }

    /// Decodes [`Header`] as [`HeaderBytes`].
    ///
    /// Returns header data encoded as a sequence of bytes.
    pub fn decode(&self) -> HeaderBytes {
        let mut header_bytes = HeaderBytes {
            size: self.size(),
            ..Default::default()
        };
        self.dump_bytes(&mut header_bytes);
        header_bytes
    }

    /// Attempts to transform header into its [`Versioned`] version.
    pub fn try_versioned<Version: Versioned>(self, version: Version) -> Result<Header<Version>> {
        version.expect(self.mavlink_version)?;

        Ok(Header {
            mavlink_version: version.mavlink_version(),
            payload_length: self.payload_length,
            incompat_flags: self.incompat_flags,
            compat_flags: self.compat_flags,
            sequence: self.sequence,
            system_id: self.system_id,
            component_id: self.component_id,
            message_id: self.message_id,
            _marker_version: PhantomData,
        })
    }

    /// Forget about header's version transforming it into a [`Versionless`] variant.
    pub fn versionless(self) -> Header<Versionless> {
        Header {
            mavlink_version: self.mavlink_version,
            payload_length: self.payload_length,
            incompat_flags: self.incompat_flags,
            compat_flags: self.compat_flags,
            sequence: self.sequence,
            system_id: self.system_id,
            component_id: self.component_id,
            message_id: self.message_id,
            _marker_version: PhantomData,
        }
    }

    pub(super) fn send<W: Write>(&self, writer: &mut W) -> Result<usize> {
        writer.write_all(self.decode().as_slice())?;
        Ok(self.size())
    }

    #[cfg(feature = "tokio")]
    pub(super) async fn send_async<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<usize> {
        writer.write_all(self.decode().as_slice()).await?;
        Ok(self.size())
    }

    fn dump_bytes(&self, header_bytes: &mut HeaderBytes) {
        match self.mavlink_version {
            MavLinkVersion::V1 => self.dump_v1_bytes(header_bytes),
            MavLinkVersion::V2 => self.dump_v2_bytes(header_bytes),
        };
    }

    fn dump_v1_bytes(&self, header_bytes: &mut HeaderBytes) {
        header_bytes.buffer[0] = MavSTX::V1.into();
        header_bytes.buffer[1] = self.payload_length;
        header_bytes.buffer[2] = self.sequence;
        header_bytes.buffer[3] = self.system_id;
        header_bytes.buffer[4] = self.component_id;
        header_bytes.buffer[5] = self.message_id.to_le_bytes()[0];
    }

    fn dump_v2_bytes(&self, header_bytes: &mut HeaderBytes) {
        let message_id: [u8; 4] = self.message_id.to_le_bytes();

        header_bytes.buffer[0] = MavSTX::V2.into();
        header_bytes.buffer[1] = self.payload_length;
        header_bytes.buffer[2] = self.incompat_flags;
        header_bytes.buffer[3] = self.compat_flags;
        header_bytes.buffer[4] = self.sequence;
        header_bytes.buffer[5] = self.system_id;
        header_bytes.buffer[6] = self.component_id;
        header_bytes.buffer[7..10].copy_from_slice(&message_id[0..3]);
    }
}

impl<V: MaybeVersioned> Header<V> {
    pub(super) fn recv<R: Read>(reader: &mut R) -> Result<Header<V>> {
        loop {
            let mut buffer = [0u8; HEADER_MIN_SIZE];
            reader.read_exact(&mut buffer)?;

            if let Some(mut header_start) = HeaderStart::<V>::from_slice(&buffer) {
                if !header_start.is_complete() {
                    reader.read_exact(header_start.remaining_bytes_mut())?;
                }
                return Header::<V>::try_from_slice(header_start.header_bytes());
            } else {
                continue;
            }
        }
    }

    #[cfg(feature = "tokio")]
    pub(super) async fn recv_async<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Header<V>> {
        loop {
            let mut buffer = [0u8; HEADER_MIN_SIZE];
            reader.read_exact(&mut buffer).await?;

            if let Some(mut header_start) = HeaderStart::<V>::from_slice(&buffer) {
                if !header_start.is_complete() {
                    reader
                        .read_exact(header_start.remaining_bytes_mut())
                        .await?;
                }
                return Header::<V>::try_from_slice(header_start.header_bytes());
            } else {
                continue;
            }
        }
    }

    fn try_from_slice(bytes: &[u8]) -> Result<Header<V>> {
        let reader = TBytesReader::from(bytes);

        let magic: u8 = reader.read()?;
        let mavlink_version: MavLinkVersion = MavLinkVersion::try_from(MavSTX::from(magic))?;
        let payload_length: u8 = reader.read()?;

        let (incompat_flags, compat_flags) = if let MavLinkVersion::V2 = mavlink_version {
            let incompat_flags = reader.read()?;
            let compat_flags = reader.read()?;
            (incompat_flags, compat_flags)
        } else {
            (0, 0)
        };

        let sequence: u8 = reader.read()?;
        let system_id: u8 = reader.read()?;
        let component_id: u8 = reader.read()?;

        let message_id: MessageId = match mavlink_version {
            MavLinkVersion::V1 => {
                let version: u8 = reader.read()?;
                version as MessageId
            }
            MavLinkVersion::V2 => {
                let version_byte: [u8; 4] = [reader.read()?, reader.read()?, reader.read()?, 0];
                MessageId::from_le_bytes(version_byte)
            }
        };

        let mut header_bytes = [0u8; HEADER_MAX_SIZE];
        header_bytes[0..bytes.len()].copy_from_slice(bytes);

        Ok(Header {
            mavlink_version,
            payload_length,
            incompat_flags,
            compat_flags,
            sequence,
            system_id,
            component_id,
            message_id,
            _marker_version: PhantomData,
        })
    }
}

impl Header<V2> {
    /// Incompatibility flags for `MAVLink 2` header.
    ///
    /// Flags that must be understood for MAVLink compatibility (implementation discards packet if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 incompatibility flags](https://mavlink.io/en/guide/serialization.html#incompat_flags).
    #[inline]
    pub fn incompat_flags(&self) -> IncompatFlags {
        self.incompat_flags
    }

    /// Compatibility flags for `MAVLink 2` header.
    ///
    /// Flags that can be ignored if not understood (implementation can still handle packet even if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 compatibility flags](https://mavlink.io/en/guide/serialization.html#compat_flags).
    #[inline]
    pub fn compat_flags(&self) -> CompatFlags {
        self.compat_flags
    }
}

impl Header<Versionless> {
    /// Initiates builder for [`Header`].
    ///
    /// Instead of constructor we use
    /// [builder](https://rust-unofficial.github.io/patterns/patterns/creational/builder.html)
    /// pattern. An instance of [`HeaderBuilder`] returned by this function is initialized
    /// with default values. Once desired values are set, you can call [`HeaderBuilder::build`]
    /// to obtain [`Header`].
    pub fn builder(
    ) -> HeaderBuilder<Versionless, NoPayloadLen, NotSequenced, NoSysId, NoCompId, NoMsgId> {
        HeaderBuilder::new()
    }

    /// Incompatibility flags for `MAVLink 2` header.
    ///
    /// Flags that must be understood for MAVLink compatibility (implementation discards packet if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 incompatibility flags](https://mavlink.io/en/guide/serialization.html#incompat_flags).
    #[inline]
    pub fn incompat_flags(&self) -> Option<IncompatFlags> {
        match self.mavlink_version() {
            MavLinkVersion::V1 => None,
            MavLinkVersion::V2 => Some(self.incompat_flags),
        }
    }

    /// Compatibility flags for `MAVLink 2` header.
    ///
    /// Flags that can be ignored if not understood (implementation can still handle packet even if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 compatibility flags](https://mavlink.io/en/guide/serialization.html#compat_flags).
    #[inline]
    pub fn compat_flags(&self) -> Option<CompatFlags> {
        match self.mavlink_version() {
            MavLinkVersion::V1 => None,
            MavLinkVersion::V2 => Some(self.compat_flags),
        }
    }
}

struct HeaderStart<V: MaybeVersioned> {
    buffer: [u8; HEADER_MAX_SIZE],
    n_bytes_read: usize,
    n_bytes_left: usize,
    _marker_version: PhantomData<V>,
}

impl<V: MaybeVersioned> HeaderStart<V> {
    fn from_slice(buffer: &[u8]) -> Option<Self> {
        let mut mavlink_version: Option<MavLinkVersion> = None;
        let mut header_start_idx = buffer.len();
        for (i, &byte) in buffer.iter().enumerate() {
            if V::is_magic_byte(byte) {
                header_start_idx = i;
                mavlink_version = MavLinkVersion::try_from(MavSTX::from(byte)).ok();
            }
        }

        match mavlink_version {
            None => None,
            Some(version) => {
                let header_size = match version {
                    MavLinkVersion::V1 => HEADER_V1_SIZE,
                    MavLinkVersion::V2 => HEADER_V2_SIZE,
                };

                let n_bytes_read = buffer.len() - header_start_idx;
                let header_start_bytes = &buffer[header_start_idx..buffer.len()];

                let mut header_bytes = [0u8; HEADER_MAX_SIZE];
                header_bytes[0..n_bytes_read].copy_from_slice(header_start_bytes);

                Some(Self {
                    buffer: header_bytes,
                    n_bytes_read,
                    n_bytes_left: header_size - n_bytes_read,
                    _marker_version: PhantomData,
                })
            }
        }
    }

    fn header_bytes(&self) -> &[u8] {
        &self.buffer[0..self.n_bytes_read + self.n_bytes_left]
    }

    fn is_complete(&self) -> bool {
        self.n_bytes_left == 0
    }

    fn remaining_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[self.n_bytes_read..self.n_bytes_read + self.n_bytes_left]
    }
}

#[cfg(test)]
mod header_tests {
    use crate::consts::{STX_V1, STX_V2};
    use crate::protocol::V1;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn read_v1_header() {
        let mut buffer = Cursor::new(vec![
            12,     // \
            24,     //  | Junk bytes
            240,    // /
            STX_V1, // magic byte
            8,      // payload_length
            1,      // sequence
            10,     // system ID
            255,    // component ID
            0,      // message ID
        ]);

        let header = Header::<V1>::recv(&mut buffer).unwrap();
        let header = header.try_versioned(V1).unwrap();

        assert!(header.try_versioned(V2).is_err());
        assert!(matches!(header.mavlink_version(), MavLinkVersion::V1));

        assert_eq!(header.payload_length(), 8u8);
        assert_eq!(header.sequence(), 1u8);
        assert_eq!(header.system_id(), 10u8);
        assert_eq!(header.component_id(), 255u8);
        assert_eq!(header.message_id(), 0u32);
    }

    #[test]
    fn read_v2_header() {
        let mut reader = Cursor::new(vec![
            12,     // \
            24,     //  |Junk bytes
            240,    // /
            STX_V2, // magic byte
            8,      // payload_length
            1,      // incompatibility flags
            0,      // compatibility flags
            1,      // sequence
            10,     // system ID
            255,    // component ID
            0,      // \
            0,      //  | message ID
            0,      // /
        ]);

        let header = Header::<Versionless>::recv(&mut reader).unwrap();
        let header = header.try_versioned(V2).unwrap();

        assert!(header.try_versioned(V1).is_err());
        assert!(matches!(header.mavlink_version(), MavLinkVersion::V2));

        assert_eq!(header.payload_length(), 8u8);
        assert_eq!(header.incompat_flags(), 1u8);
        assert_eq!(header.compat_flags(), 0u8);
        assert_eq!(header.sequence(), 1u8);
        assert_eq!(header.system_id(), 10u8);
        assert_eq!(header.component_id(), 255u8);
        assert_eq!(header.message_id(), 0u32);
    }

    #[test]
    fn build_v1_header() {
        let header = Header::builder()
            .payload_length(10)
            .sequence(5)
            .system_id(10)
            .component_id(240)
            .message_id(42)
            .mavlink_version(V1)
            .versioned();

        assert!(matches!(header.mavlink_version(), MavLinkVersion::V1));
        assert_eq!(header.payload_length(), 10);
        assert_eq!(header.sequence(), 5);
        assert_eq!(header.system_id(), 10);
        assert_eq!(header.component_id(), 240);
        assert_eq!(header.message_id(), 42);
    }

    #[test]
    fn build_v2_header() {
        let header = Header::builder()
            .incompat_flags(MAVLINK_IFLAG_SIGNED)
            .compat_flags(8)
            .payload_length(10)
            .sequence(5)
            .system_id(10)
            .component_id(240)
            .message_id(42)
            .signed(true)
            .versioned();

        assert!(matches!(header.mavlink_version(), MavLinkVersion::V2));
        assert_eq!(header.incompat_flags(), MAVLINK_IFLAG_SIGNED);
        assert_eq!(header.compat_flags(), 8);
        assert_eq!(header.payload_length(), 10);
        assert_eq!(header.sequence(), 5);
        assert_eq!(header.system_id(), 10);
        assert_eq!(header.component_id(), 240);
        assert_eq!(header.message_id(), 42);
    }
}
