use super::*;

pub type ClientId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Spawn(vec2<f32>),
    StateSync {
        babies: HashMap<ClientId, Baby>,
        until_next_race: f32,
        joining_next_race: usize,
    },
    Auth {
        id: ClientId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    StateSync(ClientState),
    Despawn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientState {
    pub baby: Option<Baby>,
    pub join_next: bool,
}
