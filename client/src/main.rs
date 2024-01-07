use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use clap::Parser;

use shared::{
    net::{
        client::{Client, ClientEvent, ClientState},
        network::{bind_socket, SERVER_PORT, NETWORK_FPS},
    },
    sim::{physics_test::PhysicsTest, GameState, Lobby, LobbyMessage},
    timing::FrameDurationAccumulator,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to expose
    #[arg(short, long, default_value_t = 4322)]
    pub port: u16,

    /// Server IP to connect to
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub server_ip: IpAddr,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    eprintln!("{:?}", args);

    let mut client = {
        let socket = {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), args.port);
            let socket = bind_socket(addr)?;
            eprintln!("Socket bound to {}", addr);
            socket
        };

        let server_addr = SocketAddr::new(args.server_ip, SERVER_PORT);

        Client::new(socket, server_addr, NETWORK_FPS)
    };

    let mut sim = FrameDurationAccumulator::with_fps(2.0, 0.25);

    let mut state = GameState::Lobby;
    let mut lobby = Lobby::new();

    let mut physics_test = PhysicsTest::new();

    loop {
        match client.process_packets() {
            Some(ClientEvent::Connected) => {
                eprintln!("connected");
                lobby.add_player(client.index); // hey, it's me!
            }
            Some(ClientEvent::ConnectionTimeout) => {
                eprintln!("disconnected");
                todo!("reconnect");
            }
            None => {}
        }

        if client.state == ClientState::Connected {
            match state {
                GameState::Lobby => {
                    while let Some(message) = client.read_new::<LobbyMessage>() {
                        match message {
                            LobbyMessage::LobbyUpdated(Lobby { join_mask }) => {
                                lobby.join_mask = join_mask;
                                print!("lobby seats: ");
                                for i in 0..8 {
                                    print!("{}", (lobby.join_mask >> i) & 1);
                                }
                                println!();
                            }
                            LobbyMessage::StartGame => {
                                println!("starting game!");
                                state = GameState::Running;
                            }
                        }
                    }
                    // sim.run_frame(|frame| {
                    //         client.write(&mut LobbyMessage::StartGame);
                    // });
                }

                GameState::Running => {
                    while let Some(message) = client.read_new::<PhysicsTest>() {
                        eprintln!("server position: {:?}", message.position);
                    }

                    sim.run_frame(|frame| {
                        physics_test.simulate(frame.dt);

                        // sync
                        {
                            client.write(&mut physics_test);
                        }

                        // debug
                        {
                            let PhysicsTest {
                                position,
                                velocity,
                                acceleration,
                                ..
                            } = physics_test;
                            println!("\tp={}\tdp={}\tddp={}", position, velocity, acceleration);
                        }
                    });

                    // Rendering
                    // let dt = simulation.interpolate_frames();
                    // let ddp = acceleration - acceleration * (1.0 - drag) * dt;
                    // let dp = velocity + ddp * dt;
                    // let p = position + dp * dt;
                    // println!("\tp={}\tdp={}\tddp={}", p, dp, ddp);
                }
            }
        }

        // NOTE: don't use all of the CPU core
        // std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
