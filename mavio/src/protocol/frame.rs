//! # MAVLink frame

use crc_any::CRCu16;

#[cfg(feature = "async")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::consts::{CHECKSUM_SIZE, SIGNATURE_LENGTH};
use crate::error::{ChecksumError, SignatureError, VersionError};
use crate::io::{Read, Write};
use crate::protocol::header::Header;
use crate::protocol::marker::{
    HasCompId, HasMsgId, HasPayload, HasPayloadLen, HasSysId, Sequenced, Unset,
};
use crate::protocol::signature::{Sign, Signature, Signer, SigningConf};
use crate::protocol::{
    Checksum, CompatFlags, ComponentId, CrcExtra, FrameBuilder, IncompatFlags, MavLinkVersion,
    MavTimestamp, MessageId, Payload, PayloadLength, SecretKey, Sequence, SignatureBytes,
    SignedLinkId, SystemId,
};

use crate::prelude::*;

/// MAVLink frame.
///
/// Represents [`MAVLink1`](https://mavlink.io/en/guide/serialization.html#v1_packet_format) or
/// [`MAVLink2`](https://mavlink.io/en/guide/serialization.html#mavlink2_packet_format) packet.
///
/// # Protocol Version
///
/// All MAVLink frames are always belong to a specific MAVLink protocol version. However, for the
/// sake of generality, this library provides a way to deal with MAVLink protocol version both
/// explicitly by operating on [`Frame<V1>`] / [`Frame<V2>`] and implicitly through
/// [`Frame<Versionless>`]. The latter can be obtained by [`Frame::into_versionless`]
/// and converted into protocol-specific form through [`Frame::try_into_versioned`]. Both versionless and
/// versioned forms of a frame could be correctly decoded into the corresponding MAVLink message.
///
/// # Encoding / Decoding
///
/// Frames can be decoded into the corresponding MAVLink message by [`Frame::decode`] method. This
/// requires appropriate MAVLink dialect to be generated by [`mavspec`]. All autogenerated dialects
/// can be found in the [`mavio::dialects`](crate::dialects) module.
///
/// To encode MAVLink message into a frame, you should use [`FrameBuilder::message`] as described in
/// [construction](#construction) section.
///
/// # Construction
///
/// Since MAVLink frames has a complex internal structure that depends on [`MavLinkVersion`],
/// encoded [`Message`] and presence of [`Signature`], there are is direct no constructor for this
/// struct. [`Frame`] can be either received as they were sent by remote or built from [`FrameBuilder`].
///
/// Use [`Frame::builder`] to create new frames via builder and [`Frame::add_signature`] (for
/// [`Frame<V2>`] only) to sign frames.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame<V: MaybeVersioned> {
    pub(super) header: Header<V>,
    pub(super) payload: Payload,
    pub(super) checksum: Checksum,
    pub(super) signature: Option<Signature>,
}

impl Frame<Versionless> {
    /// Instantiates an empty builder for [`Frame`].
    pub fn builder(
    ) -> FrameBuilder<Versionless, Unset, Unset, Unset, Unset, Unset, Unset, Unset, Unset> {
        FrameBuilder::new()
    }
}

///////////////////////////////////////////////////////////////////////////////
//                                    ALL                                    //
///////////////////////////////////////////////////////////////////////////////
impl<V: MaybeVersioned> Frame<V> {
    /// Frame [`Header`].
    #[inline]
    pub fn header(&self) -> &Header<V> {
        &self.header
    }

    /// MAVLink protocol version defined by [`Header`].
    ///
    /// # Links
    ///
    /// * [`MavLinkVersion`]
    /// * [`Header::version`]
    /// * [`MavFrame::version`]
    #[inline]
    pub fn version(&self) -> MavLinkVersion {
        self.header.version()
    }

    /// Payload length.
    ///
    /// Indicates length of the following `payload` section. This may be affected by payload truncation.
    ///
    /// # Links
    ///
    /// * [`Header::payload_length`].
    /// * [`MavFrame::payload_length`].
    #[inline]
    pub fn payload_length(&self) -> PayloadLength {
        self.header.payload_length()
    }

    /// Packet sequence number.
    ///
    /// Used to detect packet loss. Components increment value for each message sent.
    ///
    /// # Links
    ///
    /// * [`Header::sequence`].
    /// * [`MavFrame::sequence`]
    #[inline]
    pub fn sequence(&self) -> Sequence {
        self.header.sequence()
    }

    /// System `ID`.
    ///
    /// `ID` of system (vehicle) sending the message. Used to differentiate systems on network.
    ///
    /// > Note that the broadcast address 0 may not be used in this field as it is an invalid source
    /// > address.
    ///
    /// # Links
    ///
    /// * [`Header::system_id`].
    /// * [`MavFrame::system_id`].
    #[inline]
    pub fn system_id(&self) -> SystemId {
        self.header.system_id()
    }

    /// Component `ID`.
    ///
    /// `ID` of component sending the message. Used to differentiate components in a system (e.g.
    /// autopilot and a camera). Use appropriate values in
    /// [MAV_COMPONENT](https://mavlink.io/en/messages/common.html#MAV_COMPONENT).
    ///
    /// > Note that the broadcast address `MAV_COMP_ID_ALL` may not be used in this field as it is
    /// > an invalid source address.
    ///
    /// # Links
    ///
    /// * [`Header::component_id`].
    /// * [`MavFrame::component_id`].
    #[inline]
    pub fn component_id(&self) -> ComponentId {
        self.header.component_id()
    }

    /// Message `ID`.
    ///
    /// `ID` of MAVLink message. Defines how payload will be encoded and decoded.
    ///
    /// # Links
    ///
    /// * [`Header::message_id`].
    /// * [`MavFrame::message_id`].
    #[inline]
    pub fn message_id(&self) -> MessageId {
        self.header.message_id()
    }

    /// Payload data.
    ///
    /// Message data. Content depends on message type (i.e. `message_id`).
    ///
    /// Returns an instance of [`Payload`]. If you are interested in payload bytes, use
    /// [`Payload::bytes`].
    ///
    /// # Links
    ///
    /// * [`MavFrame::payload`].
    #[inline]
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    /// MAVLink packet checksum.
    ///
    /// `CRC-16/MCRF4XX` [checksum](https://mavlink.io/en/guide/serialization.html#checksum) for
    /// message (excluding magic byte).
    ///
    /// Includes [CRC_EXTRA](https://mavlink.io/en/guide/serialization.html#crc_extra) byte.
    ///
    /// Checksum is encoded with little endian (low byte, high byte).
    ///
    /// # Links
    ///
    /// * [`Frame::calculate_crc`] for implementation.
    /// * [MAVLink checksum definition](https://mavlink.io/en/guide/serialization.html#checksum).
    /// * [CRC-16/MCRF4XX](https://ww1.microchip.com/downloads/en/AppNotes/00752a.pdf) (PDF).
    /// * [`MavFrame::checksum`]
    #[inline]
    pub fn checksum(&self) -> Checksum {
        self.checksum
    }

    /// Returns `true` if frame is signed.
    ///
    /// Returns `true` if [`Frame`] contains [`Signature`]. Correctness of signature is not validated.
    ///
    /// For `MAVLink 1` frames always returns `false`.
    ///
    /// # Links
    ///
    /// * [`MavFrame::is_signed`]
    #[inline]
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    /// `MAVLink 2` signature.
    ///
    /// Returns signature that ensures the link is tamper-proof.
    ///
    /// Available only for signed `MAVLink 2` frames. For `MAVLink 1` always return `None`.
    ///
    /// # Links
    ///
    /// * [`Frame::is_signed`] checks that frame is signed.
    /// * [`Frame::link_id`] and [`Frame::timestamp`] provide direct access to signature fields
    ///   ([`Frame<V2>`] only).
    /// # [`MavFrame::signature`].
    /// * [MAVLink 2 message signing](https://mavlink.io/en/guide/message_signing.html).
    #[inline]
    pub fn signature(&self) -> Option<&Signature> {
        if self.matches_version(V2) {
            self.signature.as_ref()
        } else {
            None
        }
    }

    /// Removes `MAVLink 2` signature from frame.
    ///
    /// Applicable only for `MAVLink 2` frames. `MAVLink 1` frames will be kept untouched.
    ///
    /// # Links
    ///
    /// [`MavFrame::remove_signature`]
    pub fn remove_signature(&mut self) {
        self.signature = None;
        self.header.set_is_signed(false)
    }

    /// Body length.
    ///
    /// Returns the length of the entire [`Frame`] body. The frame body consist of [`Payload::bytes`], [`Checksum`], and
    /// optional [`Signature`] (for `MAVLink 2` protocol).
    ///
    /// # Links
    ///
    /// * [`Header::body_length`].
    /// * [`MavFrame::body_length`].
    #[inline]
    pub fn body_length(&self) -> usize {
        self.header().body_length()
    }

    /// Calculates CRC for frame within `crc_extra`.
    ///
    /// Provided `crc_extra` depends on a dialect and contains a digest of message XML definition.
    ///
    /// # Links
    ///
    /// * [`Frame::checksum`].
    /// * [`MavFrame::calculate_crc`].
    /// * [MAVLink checksum definition](https://mavlink.io/en/guide/serialization.html#checksum).
    /// * [CRC-16/MCRF4XX](https://ww1.microchip.com/downloads/en/AppNotes/00752a.pdf) (PDF).
    pub fn calculate_crc(&self, crc_extra: CrcExtra) -> Checksum {
        let mut crc_calculator = CRCu16::crc16mcrf4cc();

        crc_calculator.digest(self.header.decode().crc_data());
        crc_calculator.digest(self.payload.bytes());

        crc_calculator.digest(&[crc_extra]);

        crc_calculator.get_crc()
    }

    /// Validates frame in the context of specific dialect.
    ///
    /// Receives dialect specification in `dialect_spec`, ensures that message with such ID
    /// exists in this dialect, and compares checksums using `EXTRA_CRC`.
    ///
    /// # Errors
    ///
    /// * Returns [`Error::Spec`] if message discovery failed.  
    /// * Returns [`FrameError::Checksum`] (wrapped by [`Error`]) if checksum
    ///   validation failed.
    ///
    /// # Links
    ///
    /// * [`Dialect`] for dialect specification.
    /// * [`Frame::calculate_crc`] for CRC implementation details.
    /// * [`MavFrame::validate_checksum`].
    pub fn validate_checksum<D: Dialect>(&self) -> Result<()> {
        let message_info = D::message_info(self.header().message_id())?;
        self.validate_checksum_with_crc_extra(message_info.crc_extra())?;

        Ok(())
    }

    /// Validates frame's checksum using provided `crc_extra`.
    ///
    /// # Errors
    ///
    /// Returns [`ChecksumError`] if checksum validation failed.
    ///
    /// # Links
    ///
    /// * [`Frame::calculate_crc`] for CRC implementation details.
    /// * [`MavFrame::validate_checksum_with_crc_extra`].
    pub fn validate_checksum_with_crc_extra(
        &self,
        crc_extra: CrcExtra,
    ) -> core::result::Result<(), ChecksumError> {
        if self.calculate_crc(crc_extra) != self.checksum {
            return Err(ChecksumError);
        }

        Ok(())
    }

    /// Checks that frame has MAVLink version equal to the provided one.
    ///
    /// # Links
    ///
    /// # [`MavFrame::matches_version`]
    pub fn matches_version<Version: Versioned>(
        &self,
        #[allow(unused_variables)] version: Version,
    ) -> bool {
        Version::matches(self.version())
    }

    /// Attempts to transform frame into its versioned form.
    ///
    /// This method never changes the internal MAVLink protocol version. It will return an error,
    /// if conversion is not possible.
    pub fn try_into_versioned<Version: MaybeVersioned>(
        self,
    ) -> core::result::Result<Frame<Version>, VersionError> {
        Version::expect(self.version())?;

        Ok(Frame {
            header: self.header.try_into_versioned::<Version>()?,
            payload: self.payload,
            checksum: self.checksum,
            signature: self.signature,
        })
    }

    /// Attempts to create frame of specified version from the existing one.
    ///
    /// This method never changes the internal MAVLink protocol version. It will return an error,
    /// if conversion is not possible.
    pub fn try_to_versioned<Version: MaybeVersioned>(
        &self,
    ) -> core::result::Result<Frame<Version>, VersionError> {
        self.clone().try_into_versioned()
    }

    /// Forget about frame's version transforming it into a [`Versionless`] variant.
    pub fn into_versionless(self) -> Frame<Versionless> {
        Frame {
            header: self.header.into_versionless(),
            payload: self.payload,
            checksum: self.checksum,
            signature: self.signature,
        }
    }

    /// Forget about frame's version transforming it into a [`Versionless`] variant.
    pub fn to_versionless(&self) -> Frame<Versionless> {
        self.clone().into_versionless()
    }

    /// Converts this frame into a dynamic [`MavFrame`].
    pub fn into_mav_frame(self) -> MavFrame {
        MavFrame::new(self)
    }

    /// Decodes frame into a message of particular MAVLink dialect.
    ///
    /// Performs [`Frame::checksum`] validation before returning decoded message.
    ///
    /// # Usage
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "minimal")] {
    /// # use minimal::messages::Heartbeat;
    /// # use mavio::protocol::{V2};
    /// use mavio::dialects::minimal;
    /// use mavio::dialects::minimal::Minimal;
    /// use mavio::Frame;
    ///
    /// let frame = // ... obtain a frame
    /// #     Frame::builder()
    /// #         .sequence(0)
    /// #         .system_id(1)
    /// #         .component_id(1)
    /// #         .version(V2)
    /// #         .message(&Heartbeat::default()).unwrap()
    /// #         .build();
    ///
    /// // Decode the frame within `minimal` dialect and match result over available dialect messages
    /// match frame.decode().unwrap() {
    ///     Minimal::ProtocolVersion(_) => {}
    ///     Minimal::Heartbeat(_) => {}
    /// }
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// * Returns [`FrameError::Checksum`] if checksum validation failed.
    /// * Returns [`Error::Spec`] if frame can't be correctly decoded to the provided
    ///   [`Dialect`] (generic type argument).
    ///
    /// # Links
    ///
    /// * [`Frame::validate_checksum_with_crc_extra`] performs checksum validation.
    /// * [`SpecError`] contains errors related to MAVLink dialect specification and message
    ///   encoding/decoding.
    /// * [`Frame::decode`]
    #[inline]
    pub fn decode<D: Dialect>(&self) -> Result<D> {
        let message = D::decode(self.payload()).map_err(Error::from)?;
        self.validate_checksum_with_crc_extra(message.crc_extra())?;
        Ok(message)
    }

    /// Upgrades a frame in-place to `MAVLink 2` protocol version using provided `CRC_EXTRA`.
    ///
    /// The opposite is not possible since we need to know exact payload length.
    pub fn upgrade_with_crc_extra(&mut self, crc_extra: CrcExtra) {
        self.payload.upgrade();
        self.header.payload_length = self.payload.length();
        self.checksum = self.calculate_crc(crc_extra);
    }

    pub(crate) fn recv<R: Read>(reader: &mut R) -> Result<Frame<V>> {
        let header = Header::<V>::recv(reader)?;
        let body_length = header.body_length();

        #[cfg(feature = "std")]
        let mut body_buf = vec![0u8; body_length];
        #[cfg(not(feature = "std"))]
        let mut body_buf = [0u8; crate::consts::PAYLOAD_MAX_SIZE + SIGNATURE_LENGTH];
        let body_bytes = &mut body_buf[0..body_length];

        reader.read_exact(body_bytes)?;
        let frame = Self::try_from_raw_body(header, body_bytes)?;

        Ok(frame)
    }

    #[cfg(feature = "async")]
    pub(crate) async fn recv_async<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Frame<V>> {
        let header = Header::<V>::recv_async(reader).await?;
        let body_length = header.body_length();

        #[cfg(feature = "std")]
        let mut body_buf = vec![0u8; body_length];
        #[cfg(not(feature = "std"))]
        let mut body_buf = [0u8; crate::consts::PAYLOAD_MAX_SIZE + SIGNATURE_LENGTH];
        let body_bytes = &mut body_buf[0..body_length];

        reader.read_exact(body_bytes).await?;
        let frame = Self::try_from_raw_body(header, body_bytes)?;

        Ok(frame)
    }

    pub(crate) fn send<W: Write>(&self, writer: &mut W) -> Result<usize> {
        let header_bytes_sent = self.header.send(writer)?;

        #[cfg(not(feature = "alloc"))]
        let mut buf = [0u8; crate::consts::PAYLOAD_MAX_SIZE + SIGNATURE_LENGTH];
        #[cfg(feature = "alloc")]
        let mut buf = alloc::vec![0u8; self.body_length()];

        self.fill_body_buffer(&mut buf);
        writer.write_all(buf.as_slice())?;

        Ok(header_bytes_sent + self.body_length())
    }

    #[cfg(feature = "async")]
    pub(crate) async fn send_async<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<usize> {
        let header_bytes_sent = self.header.send_async(writer).await?;

        #[cfg(not(feature = "alloc"))]
        let mut buf = [0u8; crate::consts::PAYLOAD_MAX_SIZE + SIGNATURE_LENGTH];
        #[cfg(feature = "alloc")]
        let mut buf = vec![0u8; self.body_length()];

        self.fill_body_buffer(&mut buf);
        writer.write_all(buf.as_slice()).await?;

        Ok(header_bytes_sent + self.body_length())
    }

    fn fill_body_buffer(&self, buf: &mut [u8]) {
        let payload_length = self.payload_length() as usize;

        buf[0..payload_length].copy_from_slice(self.payload.bytes());

        let checksum_bytes: [u8; 2] = self.checksum.to_le_bytes();
        buf[payload_length..payload_length + 2].copy_from_slice(&checksum_bytes);

        if let Some(signature) = self.signature {
            let signature_bytes: SignatureBytes = signature.to_byte_array();
            let sig_start_idx = payload_length + 2;
            buf[sig_start_idx..self.body_length()].copy_from_slice(&signature_bytes);
        }
    }

    #[inline]
    fn try_from_raw_body(header: Header<V>, body_bytes: &[u8]) -> Result<Frame<V>> {
        let payload_bytes = &body_bytes[0..header.payload_length() as usize];
        let payload = Payload::new(header.message_id(), payload_bytes, header.version());

        let checksum_start = header.payload_length() as usize;
        let checksum_bytes = [body_bytes[checksum_start], body_bytes[checksum_start + 1]];
        let checksum: Checksum = Checksum::from_le_bytes(checksum_bytes);

        let signature: Option<Signature> = if header.is_signed() {
            let signature_start = checksum_start + CHECKSUM_SIZE;
            let signature_bytes = &body_bytes[signature_start..signature_start + SIGNATURE_LENGTH];
            Some(Signature::from_slice(signature_bytes))
        } else {
            None
        };

        Ok(Frame {
            header,
            payload,
            checksum,
            signature,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////
//                                   V1/V2                                   //
///////////////////////////////////////////////////////////////////////////////
impl<V: Versioned> Frame<V> {
    /// Create [`FrameBuilder`] populated with current frame data.
    ///
    /// It is not possible to simply change a particular frame field since MAVLink frame data is
    /// tightly packed together, covered by CRC, and, in the case of `MAVLink 2` protocol, is
    /// potentially signed. Moreover, to alter a frame correctly we need a [`CrcExtra`] byte which
    /// is a part of a dialect, not the frame itself.
    ///
    /// This method provides a limited capability to alter frame data by creating a [`FrameBuilder`]
    /// populated with data of the current frame. For `MAVLink 2` frames this will drop frame's
    /// [`signature`](Frame::signature) and [`IncompatFlags::MAVLINK_IFLAG_SIGNED`] in
    /// [`incompat_flags`](Frame::incompat_flags) rendering frame unsigned. This process also
    /// requires from caller to provide a [`CrcExtra`] value to encoded message since
    /// [`checksum`](Frame::checksum) will be dropped as well and the information required to its
    /// this recalculation is not stored within MAVLink frame itself.
    ///
    /// It is not possible to rebuild [`Versionless`] frames since `MAVLink 2` [`Payload`] may
    /// contain extension fields and its trailing zero bytes are truncated which means it is not
    /// possible to reconstruct `MAVLink 1` [`payload_length`](Frame::payload_length) when
    /// downgrading frame protocol version.
    pub fn to_builder(
        &self,
    ) -> FrameBuilder<
        V,
        HasPayloadLen,
        Sequenced,
        HasSysId,
        HasCompId,
        HasMsgId,
        HasPayload,
        Unset,
        Unset,
    > {
        FrameBuilder {
            header_builder: self.header.to_builder(),
            payload: HasPayload(self.payload.clone()),
            crc_extra: Unset,
            signature: Unset,
        }
    }
}

///////////////////////////////////////////////////////////////////////////////
//                                    V2                                     //
///////////////////////////////////////////////////////////////////////////////
impl Frame<V2> {
    /// Incompatibility flags for `MAVLink 2` header.
    ///
    /// Flags that must be understood for MAVLink compatibility (implementation discards packet if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 incompatibility flags](https://mavlink.io/en/guide/serialization.html#incompat_flags).
    #[inline]
    pub fn incompat_flags(&self) -> IncompatFlags {
        self.header.incompat_flags()
    }

    /// Compatibility flags for `MAVLink 2` header.
    ///
    /// Flags that can be ignored if not understood (implementation can still handle packet even if
    /// it does not understand flag).
    ///
    /// See: [MAVLink 2 compatibility flags](https://mavlink.io/en/guide/serialization.html#compat_flags).
    #[inline]
    pub fn compat_flags(&self) -> CompatFlags {
        self.header.compat_flags()
    }

    /// `MAVLink 2` signature `link_id`, an 8-bit identifier of a MAVLink channel.
    ///
    /// Peers may have different semantics or rules for different links. For example, some links may have higher
    /// priority over another during routing. Or even different secret keys for authorization.
    ///
    /// Available only for signed `MAVLink 2` frame. For `MAVLink 1` always return `None`.
    ///
    /// # Links
    ///
    /// * [`Self::signature`] from which [`Signature`] can be obtained. The former contains all signature-related fields
    ///   (if applicable).
    /// * [MAVLink 2 message signing](https://mavlink.io/en/guide/message_signing.html).
    pub fn link_id(&self) -> Option<SignedLinkId> {
        self.signature.map(|sig| sig.link_id)
    }

    /// `MAVLink 2` signature [`MavTimestamp`], a 48-bit value that specifies the moment when message was sent.
    ///
    /// The unit of measurement is the number of millisecond * 10 since MAVLink epoch (1st January 2015 GMT).
    ///
    /// According to MAVLink protocol, the sender must guarantee that the next timestamp is greater than the previous
    /// one.
    ///
    /// Available only for signed `MAVLink 2` frame. For `MAVLink 1` always return `None`.
    ///
    /// # Links
    ///
    /// * [`Self::signature`] from which [`Signature`] can be obtained. The former contains all signature-related fields
    ///   (if applicable).
    /// * [`MavTimestamp`] type which has utility function for converting from and into Unix timestamp.
    /// * [Timestamp handling](https://mavlink.io/en/guide/message_signing.html#timestamp) in MAVLink documentation.
    pub fn timestamp(&self) -> Option<MavTimestamp> {
        self.signature.map(|sig| sig.timestamp)
    }

    /// Adds signature to `MAVLink 2` frame.
    ///
    /// Signs `MAVLink 2` frame with provided instance of `signer` that implements [`Sign`] trait and signature
    /// configuration specified as [`SigningConf`].
    ///
    /// # Links
    ///
    /// * [`Sign`] trait.
    /// * [`Signature`] struct which contains frame signature.
    /// * [MAVLink 2 message signing](https://mavlink.io/en/guide/message_signing.html).
    pub fn add_signature(&mut self, signer: &mut dyn Sign, conf: &SigningConf) -> &mut Self {
        conf.apply(self, signer);
        self
    }

    /// Validates frame signature using a `signer` and a secret `key`.
    ///
    /// Returns [`SignatureError`] if frame missing a signature or have an incorrect one.
    pub fn validate_signature(
        &self,
        signer: &mut dyn Sign,
        key: &SecretKey,
    ) -> core::result::Result<(), SignatureError> {
        let signature = if let Some(signature) = self.signature {
            signature
        } else {
            return Err(SignatureError);
        };
        let mut signer = Signer::new(signer);

        if !signer.validate(&self, &signature, key) {
            return Err(SignatureError);
        }

        Ok(())
    }
}

#[cfg(feature = "unsafe")]
impl<V: MaybeVersioned> TryUpdateFrom<&dyn Message> for Frame<V> {
    type Error = SpecError;

    /// <sup>`⚠`</sup>
    /// Updates a frame with the data from the provided message.
    ///
    /// Replaces the following fields, that are guaranteed to be correct:
    ///
    /// * [`Frame::message_id`]
    /// * [`Frame::payload_length`]
    /// * [`Frame::payload`]
    /// * [`Frame::checksum`]
    ///
    /// **⚠** This method will strip [`Frame::signature`]. Make sure, that you know, how to sign
    /// the updated frame afterward.
    fn try_update_from(&mut self, value: &dyn Message) -> std::result::Result<(), Self::Error> {
        self.check_try_update_from(&value)?;
        unsafe { self.update_from_unchecked(value) }
        Ok(())
    }

    fn check_try_update_from(&self, value: &&dyn Message) -> std::result::Result<(), Self::Error> {
        value.encode(self.version())?;
        Ok(())
    }

    unsafe fn update_from_unchecked(&mut self, value: &dyn Message) {
        let payload = value.encode(self.version()).unwrap();
        let crc_extra = value.crc_extra();

        self.header.message_id = payload.id();
        self.header.payload_length = payload.length();
        self.payload = payload;
        self.checksum = self.calculate_crc(crc_extra);

        self.remove_signature();
    }
}

///////////////////////////////////////////////////////////////////////////////
//                                  TESTS                                    //
///////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    #[cfg(feature = "std")]
    use std::io::Cursor;

    use crc_any::CRCu16;

    #[allow(unused_imports)]
    use crate::protocol::{V1, V2};

    #[test]
    fn crc_calculation_algorithm_accepts_sequential_digests() {
        // We just want to test that CRC algorithm is invariant in respect to the way we feed it
        // data.

        let data = [124, 12, 22, 34, 2, 148, 82, 201, 72, 0, 18, 215, 37, 63u8];
        let split_at: usize = data.len() / 2;

        // Get all data as one slice
        let mut crc_calculator_bulk = CRCu16::crc16mcrf4cc();
        crc_calculator_bulk.digest(&data);

        // Get data as two chunks sequentially
        let mut crc_calculator_seq = CRCu16::crc16mcrf4cc();
        crc_calculator_seq.digest(&data[0..split_at]);
        crc_calculator_seq.digest(&data[split_at..data.len()]);

        assert_eq!(crc_calculator_bulk.get_crc(), crc_calculator_seq.get_crc());
    }

    #[test]
    #[cfg(feature = "std")]
    fn multiple_magic_bytes_in_stream_v1() {
        use crate::consts::STX_V1;
        let seq = STX_V1;
        let reader = Cursor::new(vec![
            STX_V1, // magic byte
            8,      // payload_length
            seq,    // sequence
            10,     // system ID
            255,    // component ID
            0,      // message ID
            // -------------------------------
            1, 1, 1, 1, 1, 1, 1, 1, // Payload
            // -------------------------------
            0, 0, // Checksum (fake)
        ]);

        let mut receiver = crate::Receiver::new::<V1>(reader);
        let frame = receiver.recv().unwrap();

        assert_eq!(frame.payload_length(), 8);
        assert_eq!(frame.sequence(), seq);
        assert_eq!(frame.system_id(), 10);
        assert_eq!(frame.component_id(), 255);
    }

    #[test]
    #[cfg(feature = "std")]
    fn multiple_magic_bytes_in_stream_v2() {
        use crate::consts::STX_V2;
        let seq = STX_V2;
        let reader = Cursor::new(vec![
            STX_V2, // magic byte
            8,      // payload_length
            0,      // incompatibility flags
            0,      // compatibility flags
            seq,    // sequence
            10,     // system ID
            255,    // component ID
            0,      // \
            0,      //  | message ID
            0,      // /
            // -------------------------------
            1, 1, 1, 1, 1, 1, 1, 1, // Payload
            // -------------------------------
            0, 0, // Checksum (fake)
        ]);

        let mut receiver = crate::Receiver::new::<V2>(reader);
        let frame = receiver.recv().unwrap();

        assert_eq!(frame.payload_length(), 8);
        assert_eq!(frame.sequence(), seq);
        assert_eq!(frame.system_id(), 10);
        assert_eq!(frame.component_id(), 255);
    }

    #[cfg(feature = "minimal")]
    mod dialect_utils {
        pub(super) use crate::dialects::minimal as dialect;
        pub(super) use crate::dialects::minimal::enums::{
            MavAutopilot, MavModeFlag, MavState, MavType,
        };
        pub(super) use crate::protocol::V1;
        pub(super) use dialect::messages::Heartbeat;

        pub(super) use super::super::*;

        pub(super) fn default_incompat_flags() -> IncompatFlags {
            IncompatFlags::BIT_3 | IncompatFlags::BIT_4
        }

        pub(super) fn default_compat_flags() -> CompatFlags {
            CompatFlags::BIT_5 | CompatFlags::BIT_6
        }

        pub(super) fn default_heartbeat_message() -> Heartbeat {
            Heartbeat {
                type_: MavType::FixedWing,
                autopilot: MavAutopilot::Generic,
                base_mode: MavModeFlag::TEST_ENABLED & MavModeFlag::CUSTOM_MODE_ENABLED,
                custom_mode: 0,
                system_status: MavState::Active,
                mavlink_version: dialect::Minimal::version().unwrap_or(0),
            }
        }

        pub(super) fn default_v1_heartbeat_frame() -> Frame<V1> {
            let message = default_heartbeat_message();
            Frame::builder()
                .sequence(7)
                .system_id(22)
                .component_id(17)
                .version(V1)
                .message(&message)
                .unwrap()
                .build()
        }

        pub(super) fn default_v2_heartbeat_frame() -> Frame<V2> {
            let message = default_heartbeat_message();
            Frame::builder()
                .sequence(7)
                .system_id(22)
                .component_id(17)
                .version(V2)
                .incompat_flags(default_incompat_flags())
                .compat_flags(default_compat_flags())
                .message(&message)
                .unwrap()
                .build()
        }
    }
    #[cfg(feature = "minimal")]
    use dialect_utils::*;

    #[test]
    #[cfg(feature = "minimal")]
    #[cfg(feature = "std")]
    fn test_signing() {
        use crate::consts::SIGNATURE_SECRET_KEY_LENGTH;
        use crate::utils::MavSha256;

        let mut frame = default_v2_heartbeat_frame();
        let frame = frame.add_signature(
            &mut MavSha256::default(),
            &SigningConf {
                link_id: 0,
                timestamp: Default::default(),
                secret: [0u8; SIGNATURE_SECRET_KEY_LENGTH].into(),
            },
        );

        assert!(frame.is_signed());
    }

    #[test]
    #[cfg(feature = "minimal")]
    fn test_decoding_to_message() {
        let _: dialect::Minimal = default_v2_heartbeat_frame().decode().unwrap();
    }

    #[test]
    #[cfg(feature = "minimal")]
    #[cfg(feature = "std")]
    fn test_rebuild_frame() {
        use crate::consts::SIGNATURE_SECRET_KEY_LENGTH;
        use crate::utils::MavSha256;

        let mut frame = default_v2_heartbeat_frame();
        frame.add_signature(
            &mut MavSha256::default(),
            &SigningConf {
                link_id: 0,
                timestamp: Default::default(),
                secret: [0u8; SIGNATURE_SECRET_KEY_LENGTH].into(),
            },
        );

        let updated = frame
            .to_builder()
            .crc_extra(dialect::messages::heartbeat::spec().crc_extra())
            .build();

        assert_eq!(updated.sequence(), frame.sequence());
        assert_eq!(updated.system_id(), frame.system_id());
        assert_eq!(updated.component_id(), frame.component_id());
        assert_eq!(updated.payload_length(), frame.payload_length());
        assert_eq!(updated.payload().bytes(), frame.payload().bytes());
        assert_eq!(updated.checksum(), frame.checksum());

        assert_eq!(
            updated.incompat_flags().bits(),
            default_incompat_flags().bits()
        );
        assert_eq!(updated.compat_flags().bits(), default_compat_flags().bits());
        assert!(!updated.is_signed());
    }

    #[test]
    #[cfg(feature = "minimal")]
    fn test_upgrade_frame() {
        let expected = default_v2_heartbeat_frame();

        let upgraded = default_v1_heartbeat_frame()
            .to_builder()
            .crc_extra(dialect::messages::heartbeat::spec().crc_extra())
            .upgrade()
            .incompat_flags(default_incompat_flags())
            .compat_flags(default_compat_flags())
            .build();

        assert_eq!(upgraded.payload_length(), expected.payload_length());
        assert_eq!(upgraded.payload().bytes(), expected.payload().bytes());
        assert_eq!(upgraded.checksum(), expected.checksum());
    }

    #[test]
    #[cfg(feature = "minimal")]
    fn test_try_versioned() {
        let v1 = default_v1_heartbeat_frame();
        assert!(v1.clone().try_into_versioned::<V1>().is_ok());
        assert!(v1.clone().try_into_versioned::<V2>().is_err());
        assert!(v1.clone().try_into_versioned::<Versionless>().is_ok());

        let versionless_v1 = v1.into_versionless();
        assert!(versionless_v1.clone().try_into_versioned::<V1>().is_ok());
        assert!(versionless_v1.clone().try_into_versioned::<V2>().is_err());
        assert!(versionless_v1
            .clone()
            .try_into_versioned::<Versionless>()
            .is_ok());

        let v2 = default_v2_heartbeat_frame();
        assert!(v2.clone().try_into_versioned::<V1>().is_err());
        assert!(v2.clone().try_into_versioned::<V2>().is_ok());
        assert!(v2.clone().try_into_versioned::<Versionless>().is_ok());

        let versionless_v2 = v2.into_versionless();
        assert!(versionless_v2.clone().try_into_versioned::<V1>().is_err());
        assert!(versionless_v2.clone().try_into_versioned::<V2>().is_ok());
        assert!(versionless_v2
            .clone()
            .try_into_versioned::<Versionless>()
            .is_ok());
    }
}
