use super::*;

pub type ClientId = u64;
pub type RaceId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedRace {
    pub joined_players: Vec<ClientId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientServerState {
    pub name: String,
    pub baby: Option<Baby>,
    pub hosting_race: bool,
    pub joined: Option<ClientId>,
    pub race_id: Option<RaceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Spawn(vec2<f32>),
    StateSync {
        clients: BTreeMap<ClientId, ClientServerState>,
    },
    Auth {
        id: ClientId,
    },
    Name(String),
    RaceResult {
        rank: usize,
        time: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    StateSync(ClientState),
    StartRace,
    Despawn,
    Finish,
    Name(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientState {
    pub baby: Option<Baby>,
    pub join_race: Option<ClientId>,
    pub host_race: bool,
}
