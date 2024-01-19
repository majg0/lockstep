use std::net::{SocketAddr, UdpSocket};

use crate::{
    moving_average::MovingAverage,
    net::{
        buffer::Buffer,
        network::{
            ConnectionAcceptedPacket, PacketType, MAX_CLIENT_BYTES_PER_SECOND, PACKET_BUFFER_SIZE,
        },
        reliable_ordered::{EndpointState, ReliableOrderedDatagramEndpoint},
        stream::{ReadStream, Stream, Streamable},
    },
    timing::FrameDurationAccumulator,
};

use super::network::PRINT_NETWORK_STATS;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ClientState {
    ConnectionRequest,
    Connecting,
    Connected,
}

pub struct Client {
    pub index: u8,
    socket: UdpSocket,
    swap_buffer: Buffer,
    endpoint: ReliableOrderedDatagramEndpoint,
    timing: FrameDurationAccumulator,
    pub state: ClientState,
    pub tx_per_frame_avg: f64,
    pub rx_per_frame_avg: f64,
}

pub enum ClientEvent {
    Connected,
    ConnectionTimeout,
}

impl Client {
    pub fn new(socket: UdpSocket, server_addr: SocketAddr, fps: f64) -> Client {
        Client {
            index: 0,
            socket,
            swap_buffer: Buffer::with_capacity(PACKET_BUFFER_SIZE),
            endpoint: ReliableOrderedDatagramEndpoint::new(server_addr),
            timing: FrameDurationAccumulator::with_fps(fps, 0.25),
            state: ClientState::ConnectionRequest,
            tx_per_frame_avg: 0.,
            rx_per_frame_avg: 0.,
        }
    }

    pub fn process_packets(&mut self) -> Option<ClientEvent> {
        let mut event = None;

        self.timing.run_frame(|frame| {
            match self.endpoint.send_outstanding(&self.socket) {
                EndpointState::Ok(stats) => {
                    if PRINT_NETWORK_STATS {
                        // NOTE: this acts as a low pass filter
                        self.tx_per_frame_avg.exponential_moving_average(stats.total_bytes_sent as f64, 0.1);
                        self.rx_per_frame_avg.exponential_moving_average(stats.total_bytes_received as f64, 0.1);

                        let txps = self.tx_per_frame_avg / frame.dt;
                        let rxps = self.rx_per_frame_avg / frame.dt;

                        println!(
                            "| Bps% {prxps:5.1}↓ {ptxps:5.1}↑ | Bps {rxps:6.0}↓ {txps:6.0}↑ | B {rx:5}↓ {tx:5}↑ | {fps} fps |",
                            fps = 1. / frame.dt,
                            rx = stats.total_bytes_received,
                            tx = stats.total_bytes_sent,
                            txps = txps,
                            rxps = rxps,
                            ptxps = txps / MAX_CLIENT_BYTES_PER_SECOND * 100.,
                            prxps = rxps / MAX_CLIENT_BYTES_PER_SECOND * 100.
                        );
                    }
                }
                EndpointState::ConnectionTimeout => {
                    self.state = ClientState::ConnectionRequest;
                    assert!(event.is_none(), "we don't need a client event queue");
                    event = Some(ClientEvent::ConnectionTimeout);
                }
            }
        });

        if let Some((header, address)) =
            ReadStream(&mut self.swap_buffer).receive_packet(&self.socket)
        {
            if address == self.endpoint.address {
                match header.packet_type {
                    // NOTE: not for the client to handle
                    PacketType::ConnectionRequest => {}

                    PacketType::ConnectionAccepted
                    | PacketType::ConnectionKeepAlive
                    | PacketType::UserPayload => {
                        self.endpoint.receive_swap(header, &mut self.swap_buffer);
                    }
                }
            }
        }

        match self.state {
            ClientState::ConnectionRequest => {
                self.endpoint.create_packet(PacketType::ConnectionRequest);
                self.state = ClientState::Connecting;
            }

            ClientState::Connecting => {
                while let Some((header, mut read_stream)) = self.endpoint.peek_message() {
                    assert!(
                        header.packet_type == PacketType::ConnectionAccepted,
                        "should only receive accept packets in connecting state"
                    );
                    let accepted: ConnectionAcceptedPacket = read_stream.stream_new();
                    self.index = accepted.index;
                    self.state = ClientState::Connected;
                    println!("connected with index {}", self.index);
                    assert!(event.is_none(), "we don't need a client event queue");
                    event = Some(ClientEvent::Connected);
                    self.endpoint.mark_handled();
                }
            }

            ClientState::Connected => {}
        }

        event
    }

    pub fn read_into<T: Streamable>(&mut self, target: &mut T) -> bool {
        assert!(
            self.state == ClientState::Connected,
            "user should only read in connected state"
        );
        if let Some((header, mut read_stream)) = self.endpoint.peek_message() {
            assert!(
                header.packet_type == PacketType::UserPayload,
                "user should only read user packets, not {:?}",
                header.packet_type
            );
            read_stream.stream_with(target);
            self.endpoint.mark_handled();
            true
        } else {
            false
        }
    }

    pub fn read_new<T: Streamable>(&mut self) -> Option<T> {
        assert!(
            self.state == ClientState::Connected,
            "user should only read in connected state"
        );
        if let Some((header, mut read_stream)) = self.endpoint.peek_message() {
            assert!(
                header.packet_type == PacketType::UserPayload,
                "user should only read user packets, not {:?}",
                header.packet_type
            );
            let message: T = read_stream.stream_new();
            self.endpoint.mark_handled();
            return Some(message);
        }
        None
    }

    pub fn write<T: Streamable>(&mut self, value: &mut T) {
        self.endpoint.write_packet(PacketType::UserPayload, |w| {
            value.stream(w);
        });
    }
}
