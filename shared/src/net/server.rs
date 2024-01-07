use std::net::{SocketAddr, UdpSocket};

use crate::{
    net::{
        buffer::Buffer,
        network::{PacketType, PACKET_BUFFER_SIZE},
        reliable_ordered::ReliableOrderedDatagramEndpoint,
        stream::{ReadStream, Stream, Streamable},
    },
    timing::FrameDurationAccumulator, moving_average::MovingAverage,
};

use super::{network::ConnectionAcceptedPacket, reliable_ordered::EndpointState};

pub struct Server {
    pub capacity: usize,
    socket: UdpSocket,
    swap_buffer: Buffer,
    endpoints: Vec<Option<ReliableOrderedDatagramEndpoint>>,
    timing: FrameDurationAccumulator,
    pub bytes_per_frame_avg: f64,
}

pub enum ServerEvent {
    ClientTimeout(u8),
    ClientConnected(u8),
}

impl Server {
    pub fn new(socket: UdpSocket, max_peer_count: usize, fps: f64) -> Server {
        Server {
            capacity: max_peer_count,
            socket,
            swap_buffer: Buffer::with_capacity(PACKET_BUFFER_SIZE),
            endpoints: Vec::new(),
            timing: FrameDurationAccumulator::with_fps(fps, 0.25),
            bytes_per_frame_avg: 0.0,
        }
    }

    fn index_of(&self, address: SocketAddr) -> Option<usize> {
        self.endpoints
            .iter()
            .enumerate()
            .find(|(_, e)| e.is_some() && e.as_ref().unwrap().address == address)
            .map(|(index, _)| index)
    }

    pub fn process_packets(&mut self) -> Option<ServerEvent> {
        let mut event = None;

        self.timing.accumulate_duration();

        while self.timing.frame_available() {
            let mut bytes_sent = 0usize;

            for (index, slot) in self.endpoints.iter_mut().enumerate() {
                if let Some(endpoint) = slot {
                    match endpoint.send_outstanding(&self.socket) {
                        EndpointState::Ok(stats) => {
                            bytes_sent += stats.bytes_sent;
                        }
                        EndpointState::ConnectionTimeout => {
                            assert!(event.is_none(), "we don't need a server event queue");
                            event = Some(ServerEvent::ClientTimeout(index as u8));
                            *slot = None;
                        }
                    }
                }
            }

            // NOTE: this acts as a low pass filter
            self.bytes_per_frame_avg.exponential_moving_average(bytes_sent as f64, 0.1);

            self.timing.consume_frame();
        }

        if let Some((header, address)) =
            ReadStream(&mut self.swap_buffer).receive_packet(&self.socket)
        {
            match header.packet_type {
                PacketType::ConnectionRequest => {
                    // NOTE: this is being processed ahead of being queued, as we have no endpoint,
                    // meaning packets could arrive multiple times and out of order. This is fine,
                    // as we only process it once, and the write back is reliable.
                    // However, in the future, we will need to handle this differently, in case IP
                    // address changes over time or we need to simply handle temporary drops and
                    // reconnects.
                    if self.index_of(address).is_none() {
                        let mut index = self.capacity; // NOTE: invalid value
                        if header.seq.unwrap() == 0 {
                            if let Some(free) = self
                                .endpoints
                                .iter()
                                .enumerate()
                                .find(|(_, e)| e.is_none())
                                .map(|(index, _)| index)
                            {
                                index = free;
                                self.endpoints[free] =
                                    Some(ReliableOrderedDatagramEndpoint::new(address));
                            } else {
                                let next = self.endpoints.len();
                                if next < self.capacity {
                                    self.endpoints
                                        .push(Some(ReliableOrderedDatagramEndpoint::new(address)));
                                    index = next;
                                }
                            }
                        }

                        if index != self.capacity {
                            let endpoint = self.endpoints[index].as_mut().unwrap();
                            endpoint.receive_swap(header, &mut self.swap_buffer);
                            endpoint.mark_handled();
                            endpoint.write_packet(PacketType::ConnectionAccepted, |w| {
                                ConnectionAcceptedPacket::new(index).stream(w);
                            });
                            assert!(event.is_none(), "we don't need a server event queue");
                            event = Some(ServerEvent::ClientConnected(index as u8));
                        } else {
                            // NOTE: we silently deny for now; maybe we shouldn't, but this is simpler
                        }
                    } else {
                        // NOTE: we can safely ignore duplicate packets
                    }
                }

                // NOTE: not for the server to handle
                PacketType::ConnectionAccepted => {}

                // NOTE: if a payload arrives before a connection request, we assume to drop it, as
                // it will be resent anyway until acked
                PacketType::UserPayload | PacketType::ConnectionKeepAlive => {
                    if let Some(index) = self.index_of(address) {
                        self.endpoints[index]
                            .as_mut()?
                            .receive_swap(header, &mut self.swap_buffer);
                    };
                }
            };
        }

        event
    }

    pub fn read_into<T: Streamable>(&mut self, index: usize, target: &mut T) -> bool {
        if let Some(Some(endpoint)) = &mut self.endpoints.get_mut(index) {
            if let Some((header, mut read_stream)) = endpoint.peek_message() {
                assert!(
                    header.packet_type == PacketType::UserPayload,
                    "user should only read user packets, not {:?}",
                    header.packet_type
                );
                read_stream.stream_with(target);
                endpoint.mark_handled();
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn read_new<T: Streamable>(&mut self, index: usize) -> Option<T> {
        if let Some(Some(endpoint)) = self.endpoints.get_mut(index) {
            if let Some((header, mut read_stream)) = endpoint.peek_message() {
                assert!(
                    header.packet_type == PacketType::UserPayload,
                    "user should only read user packets, not {:?}",
                    header.packet_type
                );
                let message: T = read_stream.stream_new();
                endpoint.mark_handled();
                return Some(message);
            }
            None
        } else {
            None
        }
    }

    pub fn drop_incoming(&mut self) {
        for endpoint in self.endpoints.iter_mut().flatten() {
            while let Some(_) = endpoint.peek_message() {
                endpoint.mark_handled();
            }
        }
    }

    pub fn broadcast<T: Streamable>(&mut self, value: &mut T) {
        for endpoint in self.endpoints.iter_mut().flatten() {
            endpoint.write_packet(PacketType::UserPayload, |w| {
                value.stream(w);
            });
        }
    }
}
