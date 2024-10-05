use super::*;

pub type ClientId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Spawn(vec2<f32>),
    StateSync { babies: HashMap<ClientId, Baby> },
    Auth { id: ClientId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    StateSync { baby: Baby },
}
