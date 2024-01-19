use std::net::{SocketAddr, UdpSocket};

use crate::{
    moving_average::MovingAverage,
    net::{
        buffer::Buffer,
        network::{
            PacketType, MAX_CLIENTS, MAX_CLIENT_BYTES_PER_SECOND, PACKET_BUFFER_SIZE,
            PRINT_NETWORK_STATS,
        },
        reliable_ordered::{EndpointSendStats, ReliableOrderedDatagramEndpoint},
        stream::{ReadStream, Stream, Streamable},
    },
    timing::FrameDurationAccumulator,
};

use super::{network::ConnectionAcceptedPacket, reliable_ordered::EndpointState};

pub struct Server {
    pub capacity: usize,
    socket: UdpSocket,
    swap_buffer: Buffer,
    endpoints: Vec<Option<ReliableOrderedDatagramEndpoint>>,
    timing: FrameDurationAccumulator,
    pub tx_per_frame_avg: f64,
    pub rx_per_frame_avg: f64,
}

#[derive(Debug)]
pub enum ServerEvent {
    ClientTimeout(u8),
    ClientConnected(u8),
}

impl Server {
    pub fn new(socket: UdpSocket, max_peer_count: u8, fps: f64) -> Server {
        let capacity = max_peer_count as usize;
        let mut endpoints = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            endpoints.push(None);
        }

        Server {
            capacity,
            socket,
            swap_buffer: Buffer::with_capacity(PACKET_BUFFER_SIZE),
            endpoints,
            timing: FrameDurationAccumulator::with_fps(fps, 0.25),
            tx_per_frame_avg: 0.0,
            rx_per_frame_avg: 0.0,
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

        self.timing.run_frame(|frame| {
            let mut stats = EndpointSendStats::default();

            // TODO: no vec allocation every frame
            let mut states = Vec::with_capacity(self.capacity);

            for index in 0..self.capacity {
                let slot = &mut self.endpoints[index];

                if let Some(endpoint) = slot {
                    let state = endpoint.send_outstanding(&self.socket);

                    match state {
                        EndpointState::Ok(endpoint_stats) => {
                            stats += &endpoint_stats;
                            states.push(Some(endpoint_stats));
                        }
                        EndpointState::ConnectionTimeout => {
                            assert!(event.is_none(), "we don't need a server event queue for now; {:?}", event);
                            event = Some(ServerEvent::ClientTimeout(index as u8));
                            states.push(None);
                            *slot = None;
                        }
                    }
                } else {
                    states.push(None);
                }
            }

            if PRINT_NETWORK_STATS {
                // NOTE: this acts as a low pass filter
                self.tx_per_frame_avg.exponential_moving_average(stats.total_bytes_sent as f64, 0.1);
                self.rx_per_frame_avg.exponential_moving_average(stats.total_bytes_received as f64, 0.1);

                let txps = self.tx_per_frame_avg / frame.dt;
                let rxps = self.rx_per_frame_avg / frame.dt;
                // TODO: precompute count
                let n = self.endpoints.iter().flatten().map(|_| 1.).sum::<f64>();

                println!(
                    "| Bps% {prxps:5.1}↓ {ptxps:5.1}↑ | Bps {rxps:6.0}↓ {txps:6.0}↑ | B {rx:5}↓ {tx:5}↑ | p {nprx:2}/{prx:<2}↓ {ptx:2}↑ | peers {n}/{nmax} | rtt ms {rtt} | frame {frame:7} @ {fps}/s|",
                    frame = frame.index,
                    n = n,
                    nmax = MAX_CLIENTS,
                    fps = 1. / frame.dt,
                    rx = stats.total_bytes_received,
                    tx = stats.total_bytes_sent,
                    txps = txps,
                    rxps = rxps,
                    ptxps = if n == 0. { 0. } else { txps / n } / MAX_CLIENT_BYTES_PER_SECOND * 100.,
                    prxps = if n == 0. { 0. } else { rxps / n } / MAX_CLIENT_BYTES_PER_SECOND * 100.,
                    nprx = stats.new_packets_received,
                    prx = stats.packets_received,
                    ptx = stats.packets_created,
                    rtt = states.iter().map(|e| {
                        match e {
                            Some(stats) => format!("{:3.0}", stats.rtt_avg * 1e3),
                            None => "  x".to_string(),
                        }
                    }).collect::<Vec<_>>().join(",")
                );
            }
        });

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
                                assert!(index == self.endpoints.len(), "we're pre-pushing None to fill capacity");
                                // let next = self.endpoints.len();
                                // if next < self.capacity {
                                //     self.endpoints
                                //         .push(Some(ReliableOrderedDatagramEndpoint::new(address)));
                                //     index = next;
                                // }
                            }
                        }

                        if index != self.capacity {
                            let endpoint = self.endpoints[index].as_mut().unwrap();
                            endpoint.receive_swap(header, &mut self.swap_buffer);
                            endpoint.mark_handled();
                            endpoint.write_packet(PacketType::ConnectionAccepted, |w| {
                                ConnectionAcceptedPacket::new(index).stream(w);
                            });
                            assert!(
                                event.is_none(),
                                "we don't need a server event queue for now; {:?}",
                                event
                            );
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
            while endpoint.peek_message().is_some() {
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
