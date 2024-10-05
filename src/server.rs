use super::*;

struct State {
    next_client_id: ClientId,
    babies: HashMap<ClientId, Baby>,
}

impl State {
    fn sync_message(&self) -> ServerMessage {
        ServerMessage::StateSync {
            babies: self.babies.clone(),
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
                next_client_id: 0,
                babies: HashMap::new(),
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
            ClientMessage::StateSync { baby } => {
                let mut state = self.state.lock().unwrap();
                state.babies.insert(self.id, baby);
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
        sender.send(ServerMessage::Spawn(vec2::ZERO));
        sender.send(state.sync_message());
        Client {
            id,
            state: self.state.clone(),
            sender,
        }
    }
}
