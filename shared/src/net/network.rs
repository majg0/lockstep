use std::{
    io,
    mem::size_of,
    net::{SocketAddr, UdpSocket},
    time::Instant,
};

use crate::{
    default_array::DefaultArray,
    endian::Endian,
    net::{
        buffer::Buffer,
        stream::{Stream, Streamable},
    },
};

pub const PROTOCOL_ID: u32 = u32::from_le_bytes(*b"MAJG");
pub const PROTOCOL_VERSION: u16 = 1;
pub const SERVER_PORT: u16 = 4321;
/// NOTE: lower than PMTU limitation 548
pub const PACKET_BUFFER_SIZE: usize = 512;
/// NOTE: we can afford such a low timeout because of high fps
pub const CONNECTION_TIMEOUT_DURATION: f64 = 0.5;
pub const NETWORK_FPS: f64 = 50.0;

pub fn bind_socket(address: SocketAddr) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(address)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

pub struct ConnectionAcceptedPacket {
    pub index: u8,
}

impl ConnectionAcceptedPacket {
    pub fn new(index: usize) -> Self {
        Self { index: index as u8 }
    }
}

impl Streamable for ConnectionAcceptedPacket {
    fn stream<S: Stream>(&mut self, s: &mut S) {
        s.copy(&mut self.index);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    #[default]
    ConnectionRequest = 0,
    // ConnectionChallenge = 2,
    ConnectionAccepted = 3,
    ConnectionKeepAlive = 4,
    UserPayload = 5,
    // Disconnect = 6
}

impl PacketType {
    pub fn valid_size_range(self) -> (usize, usize) {
        match self {
            PacketType::ConnectionRequest => (16, 16),
            PacketType::ConnectionAccepted => (
                16 + size_of::<ConnectionAcceptedPacket>(),
                16 + size_of::<ConnectionAcceptedPacket>(),
            ),
            PacketType::ConnectionKeepAlive => (16, 16),
            PacketType::UserPayload => (16, 512), // TODO: upper bound
        }
    }

    pub fn invalid_size(self, size: usize) -> bool {
        let (min, max) = self.valid_size_range();
        size < min || size > max
    }
}

impl Endian for PacketType {
    fn to_le(self) -> Self {
        self
    }
    fn to_ne(self) -> Self {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NetworkSeq(u16);

impl Endian for NetworkSeq {
    fn to_le(self) -> Self {
        Self(self.0.to_le())
    }

    fn to_ne(self) -> Self {
        Self(self.0.to_ne())
    }
}

impl NetworkSeq {
    pub fn wrap(value: u16) -> Self {
        Self(value & Self::BITMASK)
    }

    pub fn unwrap(self) -> u16 {
        self.0
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn wrapping_increment(&mut self) {
        *self = Self(self.0.wrapping_add(1) & Self::BITMASK);
    }

    pub fn wrapping_sub(self, amount: u16) -> Self {
        Self(self.0.wrapping_sub(amount) & Self::BITMASK)
    }

    pub const INVALID: Self = Self(Self::OUT_OF_BOUNDS);

    /// NOTE: assuming max 50 Hz send rate and 1s timeout, and that packets fit in single buffers,
    /// this is a high enough count!
    pub const COUNT: usize = 64;
    pub const OUT_OF_BOUNDS: u16 = u16::MAX;
    pub const BITMASK: u16 = Self::COUNT as u16 - 1;
    pub const MID_VALUE: u16 = Self::COUNT as u16 >> 1;
}

impl PartialOrd for NetworkSeq {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self == other {
            Some(std::cmp::Ordering::Equal)
        } else if ((self.0 > other.0) && (self.0 - other.0 <= Self::MID_VALUE))
            || ((self.0 < other.0) && (other.0 - self.0 > Self::MID_VALUE))
        {
            Some(std::cmp::Ordering::Greater)
        } else {
            Some(std::cmp::Ordering::Less)
        }
    }
}

#[derive(Debug, Default)]
pub struct PacketHeader {
    /// crc32 checksum of whole packet with this field set to PROTOCOL_ID; must match in packet
    /// integrity check
    pub checksum: u32,
    /// protocol version, must match in packet integrity check
    pub version: u16,
    /// helps determine valid packet size
    pub packet_type: PacketType,
    /// a number that increases with each packet sent
    pub seq: NetworkSeq,
    /// the most recent packet sequence number received
    pub ack: NetworkSeq,
    /// a bitfield encoding the set of acked packets
    pub ack_bits: u32,
}

impl PacketHeader {
    pub fn new(
        packet_type: PacketType,
        seq: NetworkSeq,
        ack: NetworkSeq,
        ack_bits: u32,
    ) -> PacketHeader {
        PacketHeader {
            checksum: PROTOCOL_ID,
            version: PROTOCOL_VERSION,
            packet_type,
            seq,
            ack,
            ack_bits,
        }
    }
}

impl Streamable for PacketHeader {
    fn stream<S: Stream>(&mut self, s: &mut S) {
        s.copy(&mut self.checksum);
        s.copy(&mut self.version);
        s.copy(&mut self.packet_type);
        s.copy(&mut 0u8); // TODO: remove padding, or keep for alignment to increase decode performance?
        s.copy(&mut self.seq);
        s.copy(&mut self.ack);
        s.copy(&mut self.ack_bits);
    }
}

pub struct SendPacket {
    pub first_send_time: Option<Instant>,
    pub buffer: Buffer,
}

impl Default for SendPacket {
    fn default() -> Self {
        Self {
            first_send_time: None,
            buffer: Buffer::with_capacity(PACKET_BUFFER_SIZE),
        }
    }
}

pub struct ReceivePacket {
    pub header: PacketHeader,
    pub buffer: Buffer,
    pub handled: bool
}

impl Default for ReceivePacket {
    fn default() -> Self {
        Self {
            header: PacketHeader::default(),
            buffer: Buffer::with_capacity(PACKET_BUFFER_SIZE),
            handled: false,
        }
    }
}

pub struct SequenceBuffer<T: DefaultArray> {
    sequences: Box<[NetworkSeq; NetworkSeq::COUNT]>,
    data: Box<[T; NetworkSeq::COUNT]>,
}

impl<T: DefaultArray> Default for SequenceBuffer<T> {
    fn default() -> Self {
        Self {
            sequences: Box::new([NetworkSeq::INVALID; NetworkSeq::COUNT]),
            data: Box::new(DefaultArray::default_array()),
        }
    }
}

impl<T: DefaultArray> SequenceBuffer<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        for seq in self.sequences.iter_mut() {
            *seq = NetworkSeq::INVALID;
        }
    }

    pub fn dbg_print(&self) {
      // for i in 0..NetworkSeq::COUNT {
      //   if i % 8 == 0 {
      //     eprint!("{}", i / 8);
      //   } else if i % 4 == 0 {
      //     eprint!(":");
      //   } else {
      //     eprint!(".");
      //   }
      // }
      // eprintln!("");
      for i in 0..NetworkSeq::COUNT {
        if self.contains(NetworkSeq(i as u16)) {
          eprint!("1");
        } else {
          eprint!("0");
        }
      }
      eprintln!("");
    }

    pub fn contains(&self, seq: NetworkSeq) -> bool {
        self.sequences[seq.index()] == seq
    }

    pub fn mark_invalid(&mut self, seq: NetworkSeq) {
        self.sequences[seq.index()] = NetworkSeq::INVALID;
    }

    pub fn get(&self, seq: NetworkSeq) -> Option<&T> {
        let index = seq.index();
        if self.sequences[index] == seq {
            Some(&self.data[index])
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, seq: NetworkSeq) -> Option<&mut T> {
        let index = seq.index();
        if self.sequences[index] == seq {
            Some(&mut self.data[index])
        } else {
            None
        }
    }

    pub fn mark_valid(&mut self, seq: NetworkSeq) -> &mut T {
        let index = seq.index();
        self.sequences[index] = seq;
        &mut self.data[index]
    }
}
