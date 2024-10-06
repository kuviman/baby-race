use super::*;

#[derive(Deserialize)]
struct Config {
    race_timer: f64,
}

struct RaceState {
    finished: usize,
}

struct State {
    config: Config,
    next_race_id: RaceId,
    next_client_id: ClientId,
    races: HashMap<RaceId, RaceState>,
    clients: BTreeMap<ClientId, ClientServerState>,
}

impl State {
    fn find_new_spawn_pos(&self) -> vec2<f32> {
        let mut used_x = HashSet::new();
        for client in self.clients.values() {
            if let Some(baby) = &client.baby {
                used_x.insert(baby.pos.x.round() as i32);
            }
        }
        let unused_x = (0..)
            .flat_map(|abs| [-abs, abs])
            .find(|x| !used_x.contains(x))
            .unwrap();
        vec2(unused_x as f32, 0.0)
    }
    fn sync_message(&self) -> ServerMessage {
        ServerMessage::StateSync {
            clients: self.clients.clone(),
        }
    }
}

pub struct App {
    state: Arc<Mutex<State>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                next_race_id: 0,
                races: default(),
                config: futures::executor::block_on(file::load_detect(
                    run_dir().join("assets").join("server.toml"),
                ))
                .unwrap(),
                next_client_id: 0,
                clients: default(),
            })),
        }
    }
}

pub struct Client {
    id: ClientId,
    state: Arc<Mutex<State>>,
    sender: Box<dyn geng::net::Sender<ServerMessage>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        let _client = state.clients.remove(&self.id).unwrap();
    }
}

impl geng::net::Receiver<ClientMessage> for Client {
    fn handle(&mut self, message: ClientMessage) {
        match message {
            ClientMessage::Finish => {
                let mut state = self.state.lock().unwrap();
                let client = state.clients.get_mut(&self.id).unwrap();
                client.baby = None;
                if let Some(race_id) = client.race_id {
                    let race = state.races.get_mut(&race_id).unwrap();
                    race.finished += 1;
                    self.sender.send(ServerMessage::RaceResult {
                        rank: race.finished,
                    });
                }
            }
            ClientMessage::StartRace => {
                let mut state = self.state.lock().unwrap();
                if state.clients[&self.id].baby.is_some() {
                    return;
                }
                let participants: Vec<ClientId> = state
                    .clients
                    .iter()
                    .filter_map(|(id, client)| {
                        if client.joined == Some(self.id) || *id == self.id {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                let race_id = state.next_race_id;
                state.next_race_id += 1;
                state.races.insert(race_id, RaceState { finished: 0 });
                for id in participants {
                    let baby = Baby::new(None, state.find_new_spawn_pos());
                    let client = state.clients.get_mut(&id).unwrap();
                    client.hosting_race = false;
                    client.joined = None;
                    client.race_id = Some(race_id);
                    client.baby = Some(baby);
                }
            }
            ClientMessage::Despawn => {
                let mut state = self.state.lock().unwrap();
                let client = state.clients.get_mut(&self.id).unwrap();
                client.baby = None;
                client.joined = None;
                client.hosting_race = false;
            }
            ClientMessage::StateSync(mut update) => {
                let mut state = self.state.lock().unwrap();
                if let Some(id) = update.join_race {
                    if !state.clients.contains_key(&id) {
                        update.join_race = None;
                    }
                    update.host_race = false;
                }
                let client = state.clients.get_mut(&self.id).unwrap();
                if let Some(baby) = &mut client.baby {
                    if let Some(update) = update.baby {
                        *baby = update;
                    } else {
                        self.sender.send(ServerMessage::Spawn(baby.pos));
                    }
                } else {
                    client.joined = update.join_race;
                    client.hosting_race = update.host_race;
                }
                self.sender.send(state.sync_message());
            }
        }
    }
}

impl geng::net::server::App for App {
    type Client = Client;
    type ServerMessage = ServerMessage;
    type ClientMessage = ClientMessage;
    fn connect(
        &mut self,
        mut sender: Box<dyn geng::net::Sender<Self::ServerMessage>>,
    ) -> Self::Client {
        let mut state = self.state.lock().unwrap();
        let id = state.next_client_id;
        state.next_client_id += 1;
        state.clients.insert(
            id,
            ClientServerState {
                baby: None,
                hosting_race: false,
                joined: None,
                race_id: None,
            },
        );
        sender.send(ServerMessage::Auth { id });
        sender.send(state.sync_message());
        Client {
            id,
            state: self.state.clone(),
            sender,
        }
    }
}
