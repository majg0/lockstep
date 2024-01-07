use std::net::{SocketAddr, UdpSocket};

use crate::{
    net::{
        buffer::Buffer,
        network::{ConnectionAcceptedPacket, PacketType, PACKET_BUFFER_SIZE},
        reliable_ordered::{EndpointState, ReliableOrderedDatagramEndpoint},
        stream::{ReadStream, Stream, Streamable},
    },
    timing::FrameDurationAccumulator, moving_average::MovingAverage,
};

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
    pub bytes_per_frame_avg: f64,
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
            bytes_per_frame_avg: 0.0
        }
    }

    pub fn process_packets(&mut self) -> Option<ClientEvent> {
        let mut event = None;

        self.timing.accumulate_duration();

        while self.timing.frame_available() {
            match self.endpoint.send_outstanding(&self.socket) {
                EndpointState::Ok(stats) => {
                    // NOTE: this acts as a low pass filter
                    self.bytes_per_frame_avg.exponential_moving_average(stats.bytes_sent as f64, 0.1);
                }
                EndpointState::ConnectionTimeout => {
                    self.state = ClientState::ConnectionRequest;
                    assert!(event.is_none(), "we don't need a client event queue");
                    event = Some(ClientEvent::ConnectionTimeout);
                }
            }

            self.timing.consume_frame();
        }

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
