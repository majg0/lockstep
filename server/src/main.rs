use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Instant,
};

use shared::{
    net::{
        network::{bind_socket, SERVER_PORT, NETWORK_FPS},
        server::{Server, ServerEvent},
    },
    sim::{physics_test::PhysicsTest, GameState, Lobby, LobbyMessage},
    timing::FrameDurationAccumulator,
};

const MAX_CLIENTS: usize = 8;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut server = {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SERVER_PORT);
        let socket = bind_socket(addr)?;
        eprintln!("socket bound to {}", addr);

        Server::new(socket, MAX_CLIENTS, NETWORK_FPS)
    };

    let mut sim = FrameDurationAccumulator::with_fps(2.0, 0.25);

    let mut state = GameState::Lobby;

    let mut lobby = Lobby::new();

    let mut start_time = Instant::now();

    loop {
        if let Some(event) = server.process_packets() {
            match event {
                ServerEvent::ClientConnected(index) => {
                    lobby.add_player(index);
                }
                ServerEvent::ClientTimeout(index) => {
                    lobby.remove_player(index);
                }
            }
            start_time = Instant::now();
            server.broadcast(&mut LobbyMessage::LobbyUpdated(lobby.clone()));
            {
                print!("lobby seats: ");
                for i in 0..8 {
                    print!("{}", (lobby.join_mask >> i) & 1);
                }
                println!();
            }
        }

        match state {
            GameState::Lobby => {
                if Instant::now().duration_since(start_time).as_secs() >= 3 && lobby.join_mask >= 0b11 {
                    println!("starting");
                    server.broadcast(&mut LobbyMessage::StartGame);
                    state = GameState::Running;
                }
                server.drop_incoming();
            }
            GameState::Running => {
                sim.run_frame(|frame| {
                    eprintln!("\n==== SIM FRAME {} ====", frame.index);
                    for index in 0..server.capacity {
                        while let Some(mut data) = server.read_new::<PhysicsTest>(index) {
                            eprintln!("ep = {}; p = {}", index, data.position);
                            server.broadcast(&mut data)
                        }
                    }
                });
            }
        }

        // NOTE: don't use all of the CPU core
        // std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
