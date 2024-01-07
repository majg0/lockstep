use std::{
    io,
    mem::MaybeUninit,
    net::{SocketAddr, UdpSocket},
};

use crate::{
    endian::Endian,
    net::{
        buffer::Buffer,
        network::{NetworkSeq, PacketHeader, PacketType, PROTOCOL_ID, PROTOCOL_VERSION},
    },
};

pub trait Streamable {
    /// NOTE: implementations will compile out checks on `IS_READING` / `IS_WRITING`
    /// during optimization, after monomorphization of the generic serializer.
    fn stream<S: Stream>(&mut self, s: &mut S);
}

impl<Value: Copy + Endian> Streamable for Value {
    fn stream<S: Stream>(&mut self, s: &mut S) {
        s.copy(self);
    }
}

pub trait Stream {
    const IS_WRITING: bool;
    const IS_READING: bool;
    //fn serialize<Value: Serializable>(&mut self, value: &mut Value);
    fn copy<Value: Copy + Endian>(&mut self, value: &mut Value);
    fn write<Value: Copy + Endian>(&mut self, value: Value);
    fn read<Value: Copy + Endian>(&mut self) -> Value;

    fn stream_with<Value: Streamable>(&mut self, target: &mut Value)
    where
        Self: Sized,
    {
        target.stream(self)
    }

    fn stream_new<Value: Streamable>(&mut self) -> Value
    where
        Self: Sized,
    {
        let mut value = unsafe { MaybeUninit::<Value>::zeroed().assume_init() };
        value.stream(self);
        value
    }
}

pub struct WriteStream<'a>(pub &'a mut Buffer);

impl WriteStream<'_> {
    pub fn init_packet(
        &mut self,
        packet_type: PacketType,
        local_sequence: NetworkSeq,
        remote_ack: NetworkSeq,
        remote_ack_bits: u32,
    ) {
        self.0.reset_writer();
        PacketHeader::new(packet_type, local_sequence, remote_ack, remote_ack_bits).stream(self);
    }

    pub fn finish_packet(&mut self) {
        // NOTE: overwrite protocol id
        let checksum = crc32fast::hash(self.0.written_slice());
        self.0.write_at(checksum, 0);
    }
}

impl Stream for WriteStream<'_> {
    const IS_WRITING: bool = true;
    const IS_READING: bool = false;

    fn copy<Value: Copy + Endian>(&mut self, value: &mut Value) {
        self.0.write(*value);
    }

    fn write<Value: Copy + Endian>(&mut self, value: Value) {
        self.0.write(value);
    }

    fn read<Value: Copy + Endian>(&mut self) -> Value {
        panic!("unexpected read from write stream, did you forget to check IS_WRITING?");
    }
}

pub struct ReadStream<'a>(pub &'a mut Buffer);

impl Stream for ReadStream<'_> {
    const IS_WRITING: bool = false;
    const IS_READING: bool = true;

    fn copy<Value: Copy + Endian>(&mut self, value: &mut Value) {
        *value = self.0.read();
    }

    fn write<Value: Copy + Endian>(&mut self, _value: Value) {
        panic!("unexpected write from read stream, did you forget to check IS_READING?");
    }

    fn read<Value: Copy + Endian>(&mut self) -> Value {
        self.0.read()
    }
}

impl ReadStream<'_> {
    fn verify_incoming_packet_integrity(&mut self, header: &PacketHeader) -> bool {
        if header.packet_type.invalid_size(self.0.read_size()) {
            eprintln!(
                "WARNING: INVALID SIZE OF {:?}: {}",
                header.packet_type,
                self.0.read_size()
            );
            return false;
        }

        if header.version != PROTOCOL_VERSION {
            eprintln!("WARNING: INVALID VERSION {}", header.version);
            return false;
        }

        self.0.write_at(PROTOCOL_ID, 0); // NOTE: necessary for correct checksum

        let checksum = crc32fast::hash(self.0.read_slice());
        if checksum != header.checksum {
            eprintln!("WARNING: INVALID CHECKSUM {}", header.version);
            return false;
        }

        true
    }

    pub fn receive_packet(&mut self, socket: &UdpSocket) -> Option<(PacketHeader, SocketAddr)> {
        match socket.recv_from(self.0.full_slice_mut()) {
            Ok((num_bytes, address)) => {
                self.0.reset_reader(num_bytes);
                let header = self.stream_new();
                if self.verify_incoming_packet_integrity(&header) {
                    Some((header, address))
                } else {
                    // TODO: should bubble up errors from integrity check?
                    None
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => panic!("socket recv io error: {e}"),
        }
    }
}
