use super::*;

#[derive(Deserialize)]
struct Config {
    race_timer: f64,
}

struct State {
    config: Config,
    next_race_timer: Timer,
    next_client_id: ClientId,
    babies: HashMap<ClientId, Baby>,
    next_race: HashMap<ClientId, bool>,
}

impl State {
    fn find_new_spawn_pos(&self) -> vec2<f32> {
        vec2::ZERO
    }
    fn tick(&mut self) {
        if self.next_race_timer.elapsed().as_secs_f64() > self.config.race_timer {
            self.next_race_timer.reset();
            for (id, join) in std::mem::take(&mut self.next_race) {
                if !join {
                    continue;
                }
                let baby = Baby::new(None, self.find_new_spawn_pos());
                self.babies.insert(id, baby);
            }
        }
    }
    fn sync_message(&self) -> ServerMessage {
        ServerMessage::StateSync {
            babies: self.babies.clone(),
            until_next_race: (self.config.race_timer - self.next_race_timer.elapsed().as_secs_f64())
                .max(0.0) as f32,
            joining_next_race: self.next_race.values().filter(|&&join| join).count(),
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
                config: futures::executor::block_on(file::load_detect(
                    run_dir().join("assets").join("server.toml"),
                ))
                .unwrap(),
                next_race_timer: Timer::new(),
                next_client_id: 0,
                babies: HashMap::new(),
                next_race: HashMap::new(),
            })),
        }
    }
}

pub struct Client {
    id: ClientId,
    state: Arc<Mutex<State>>,
    sender: Box<dyn geng::net::Sender<ServerMessage>>,
}

impl geng::net::Receiver<ClientMessage> for Client {
    fn handle(&mut self, message: ClientMessage) {
        match message {
            ClientMessage::Despawn => {
                self.state.lock().unwrap().babies.remove(&self.id);
            }
            ClientMessage::StateSync(client_state) => {
                let mut state = self.state.lock().unwrap();
                state.tick();
                if let Some(client_baby) = state.babies.get_mut(&self.id) {
                    if let Some(update) = client_state.baby {
                        *client_baby = update;
                    } else {
                        self.sender.send(ServerMessage::Spawn(client_baby.pos));
                    }
                }
                state.next_race.insert(self.id, client_state.join_next);
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
        sender.send(ServerMessage::Auth { id });
        sender.send(state.sync_message());
        Client {
            id,
            state: self.state.clone(),
            sender,
        }
    }
}
