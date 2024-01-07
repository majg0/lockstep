use std::{
    net::{SocketAddr, UdpSocket},
    time::Instant,
};

use crate::{net::{
    buffer::Buffer,
    network::{NetworkSeq, PacketHeader, PacketType, ReceivePacket, SendPacket, SequenceBuffer},
    stream::{ReadStream, WriteStream},
}, moving_average::MovingAverage};

use super::network::CONNECTION_TIMEOUT_DURATION;

pub struct ReliableOrderedDatagramEndpoint {
    pub address: SocketAddr,
    send_buffer: SequenceBuffer<SendPacket>,
    first_send_seq: NetworkSeq,
    next_send_seq: NetworkSeq,
    receive_buffer: SequenceBuffer<ReceivePacket>,
    latest_receive_seq: NetworkSeq,
    first_receive_seq: NetworkSeq,
    /// average round trip time
    rtt_avg: f64,
    packets_created_since_last_send: u16,
}

pub struct EndpointSendStats {
    pub bytes_sent: usize,
}

pub enum EndpointState {
    Ok(EndpointSendStats),
    ConnectionTimeout,
}

impl ReliableOrderedDatagramEndpoint {
    pub fn new(address: SocketAddr) -> Self {
        let send_seq = NetworkSeq::wrap(0);
        let send_buffer = SequenceBuffer::new();
        Self {
            address,
            send_buffer,
            first_send_seq: send_seq,
            next_send_seq: send_seq,
            receive_buffer: SequenceBuffer::new(),
            latest_receive_seq: NetworkSeq::wrap(0),
            first_receive_seq: NetworkSeq::wrap(0),
            rtt_avg: 0.0,
            packets_created_since_last_send: 0,
        }
    }

    pub fn create_packet(&mut self, packet_type: PacketType) -> NetworkSeq {
        self.write_packet(packet_type, |_| {})
    }

    pub fn write_packet<F: FnOnce(&mut WriteStream)>(
        &mut self,
        packet_type: PacketType,
        f: F,
    ) -> NetworkSeq {
        let seq = self.next_send_seq;
        self.next_send_seq.wrapping_increment();

        let packet = self.send_buffer.mark_valid(seq);
        packet.first_send_time = None;

        let mut remote_ack_bits = 0;
        for bit in 0..32 {
            let seq = self.latest_receive_seq.wrapping_sub(bit + 1);
            if self.receive_buffer.contains(seq) {
                remote_ack_bits |= 1 << bit;
            }
        }

        let mut w = WriteStream(&mut packet.buffer);

        w.init_packet(packet_type, seq, self.latest_receive_seq, remote_ack_bits);

        f(&mut w);

        w.finish_packet();

        self.packets_created_since_last_send += 1;

        seq
    }

    pub fn send_outstanding(&mut self, socket: &UdpSocket) -> EndpointState {
        let mut bytes_sent = 0usize;
        // send unacked packets
        let current_max_packet_rtt = {
            let update_time = Instant::now();
            let mut min_send_time = update_time;

            {
                // TODO: some sort of range iterator between NetworkSeqs or even on the SequenceBuffer?
                let mut seq_iter = self.first_send_seq;

                while seq_iter != self.next_send_seq {
                    if let Some(packet) = self.send_buffer.get_mut(seq_iter) {
                        // send packet
                        {
                            let buffer = packet.buffer.written_slice();
                            match socket.send_to(buffer, self.address) {
                                Ok(_) => (),
                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                                Err(e) => panic!("socket send io error: {e}"),
                            };
                            bytes_sent += packet.buffer.written_size();
                        }

                        // find earliest outstanding send time, for rtt estimation
                        if let Some(send_time) = packet.first_send_time {
                            if send_time < min_send_time {
                                min_send_time = send_time;
                            }
                        } else {
                            packet.first_send_time = Some(Instant::now());
                            // NOTE: since we just added it, no need to set min_send_time
                        }
                    }
                    seq_iter.wrapping_increment();
                }
            }

            update_time.duration_since(min_send_time).as_millis() as f64 / 1e3
        };

        if self.packets_created_since_last_send == 0 {
            // NOTE: must do this periodically in order to keep acks going, as acks are written on
            // packet creation rather than before sending.
            self.create_packet(PacketType::ConnectionKeepAlive);
        }
        self.packets_created_since_last_send = 0;

        // TODO: configurable timeout duration
        if current_max_packet_rtt >= CONNECTION_TIMEOUT_DURATION {
            // reset
            {
                self.send_buffer.reset();
                self.receive_buffer.reset();
                self.rtt_avg = 0.0;
            }
            EndpointState::ConnectionTimeout
        } else {
            EndpointState::Ok(EndpointSendStats { bytes_sent })
        }
    }

    fn ack(&mut self, seq: NetworkSeq) {
        if let Some(packet) = self.send_buffer.get_mut(seq) {
            if let Some(first_send_time) = packet.first_send_time {
                let rtt = Instant::now().duration_since(first_send_time).as_millis() as f64 / 1e3;

                // NOTE: this acts as a low pass filter on roundtrip time:
                self.rtt_avg = self.rtt_avg.exponential_moving_average(rtt, 0.1);

                // NOTE: we reset details at creation
                self.send_buffer.mark_invalid(seq);
            }
        }
    }

    pub fn receive_swap(&mut self, header: PacketHeader, buffer: &mut Buffer) {

        // advance most recently received sequence number
        {
            let seq = &mut self.latest_receive_seq;
            while *seq < header.seq {
                // NOTE: remove old entries to avoid breaking the ack logic with false acks.
                // we can't delete immediately in front of us, as message peeking may happen before
                // receive during wrap-around.
                // we also can't delete immediately behind us, because that means we create acks
                // for those packets in the send logic.
                // 40 is because of the 32 ack bits + the seq + some nice padding.
                self.receive_buffer.mark_invalid(header.seq.wrapping_sub(40));
                seq.wrapping_increment();
            }
        }

        let ack = header.ack;
        let ack_bits = header.ack_bits;

        // NOTE: if it's NOT a duplicate, mark it valid and swap it
        if header.seq >= self.first_receive_seq {
            // NOTE: by swapping out the input buffer (32B pointing to heap memory), we retain the data
            let packet = self.receive_buffer.mark_valid(header.seq);
            std::mem::swap(buffer, &mut packet.buffer);
            packet.header = header;
        }

        // mark packets acked
        {
            self.ack(ack);
            for bit in 0..32 {
                if ack_bits & (1 << bit) != 0 {
                    let seq = ack.wrapping_sub(bit + 1);
                    self.ack(seq);
                }
            }
        }

        // move first_send_seq forward toward latest acked packet
        {
            let seq = &mut self.first_send_seq;
            while *seq < ack {
                if self.send_buffer.contains(*seq) {
                    break;
                }
                // NOTE: old entries already marked invalid during ack
                seq.wrapping_increment();
            }
        }
    }

    pub fn peek_message(&mut self) -> Option<(&PacketHeader, ReadStream)> {
        let mut found = false;

        loop {
            let message = self.receive_buffer.get_mut(self.first_receive_seq);
            if let Some(packet) = message {
                if packet.header.packet_type == PacketType::ConnectionKeepAlive {
                    self.mark_handled();
                    continue;
                }

                found = true;
                break;
            } else {
                break;
            }
        }

        if found {
            let packet = self.receive_buffer.get_mut(self.first_receive_seq).unwrap();
            return Some((&packet.header, ReadStream(&mut packet.buffer)));
        }

        None
    }

    pub fn mark_handled(&mut self) {
        // self.receive_buffer.mark_invalid(self.first_receive_seq);
        // NOTE: we're marking invalid in the receive logic instead
        // NOTE: we don't need to modify any memory, it's simply reused for another later
        self.first_receive_seq.wrapping_increment();
    }
}
