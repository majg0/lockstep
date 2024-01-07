use crate::net::stream::{Stream, Streamable};

pub mod physics_test;

pub enum GameState {
    Lobby,
    Running,
}

#[derive(Clone)]
pub struct Lobby {
    pub join_mask: u8,
}

impl Lobby {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { join_mask: 0 }
    }

    pub fn add_player(&mut self, index: u8) {
        self.join_mask |= 1 << index;
    }

    pub fn remove_player(&mut self, index: u8) {
        self.join_mask &= !(1 << index);
    }
}

impl Streamable for Lobby {
    fn stream<S: Stream>(&mut self, s: &mut S) {
        self.join_mask.stream(s);
    }
}

#[repr(u8)]
pub enum LobbyMessage {
    LobbyUpdated(Lobby) = 10,
    StartGame,
}

impl LobbyMessage {
    const fn discriminant(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }

    /// NOTE: only grabs the first byte, which is valid because of repr(u8), so only use this for
    /// matching against - the rest will contain garbage and yield undefined behaviour (=UB)!
    unsafe fn discriminate(discriminant: &u8) -> &Self {
        std::mem::transmute::<&u8, &Self>(discriminant)
    }
}

impl Streamable for LobbyMessage {
    fn stream<S: Stream>(&mut self, s: &mut S) {
        if S::IS_READING {
            match unsafe { LobbyMessage::discriminate(&s.read()) } {
                LobbyMessage::LobbyUpdated(_) => {
                    *self = LobbyMessage::LobbyUpdated(s.stream_new());
                }
                LobbyMessage::StartGame => {
                    *self = LobbyMessage::StartGame;
                }
            }
        }
        if S::IS_WRITING {
            s.write(self.discriminant());
            match self {
                LobbyMessage::LobbyUpdated(data) => {
                    data.stream(s);
                }
                LobbyMessage::StartGame => {}
            }
        }
    }
}
