use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::Server;
use ezsockets::Session;
use ezsockets::Socket;
use std::collections::HashMap;
use std::io::BufRead;
use std::net::SocketAddr;

type SessionID = u8;

#[derive(Debug)]
enum Message {
    Broadcast {
        text: String,
        exceptions: Vec<SessionID>,
    },
}

struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    handle: Server<ChatServer>,
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Message = Message;
    type Session = SessionActor;

    async fn accept(
        &mut self,
        socket: Socket,
        _address: SocketAddr,
    ) -> Result<Session, BoxError> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let handle = Session::create(
            |_handle| SessionActor {
                id,
                server: self.handle.clone(),
            },
            socket,
        );
        self.sessions.insert(id, handle.clone());
        Ok(handle)
    }

    async fn disconnected(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), BoxError> {
        assert!(self.sessions.remove(&id).is_some());
        Ok(())
    }

    async fn message(&mut self, message: Self::Message) {
        match message {
            Message::Broadcast { exceptions, text } => {
                let sessions = self
                    .sessions
                    .iter()
                    .filter(|(id, _)| !exceptions.contains(id));
                for (id, handle) in sessions {
                    tracing::info!("broadcasting {text} to {id}");
                    handle.text(text.clone()).await;
                }
            }
        };
    }
}

struct SessionActor {
    id: SessionID,
    server: Server<ChatServer>,
}

#[async_trait]
impl ezsockets::SessionExt for SessionActor {
    type ID = SessionID;

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        self.server
            .call(Message::Broadcast {
                exceptions: vec![self.id],
                text,
            })
            .await;
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), BoxError> {
        unimplemented!()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|handle| ChatServer {
        sessions: HashMap::new(),
        handle,
    })
    .await;
    tokio::spawn({
        let server = server.clone();
        async move {
            ezsockets::tungstenite::run(server, "127.0.0.1:8080")
                .await
                .unwrap();
        }
    });

    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        server
            .call(Message::Broadcast {
                text: line,
                exceptions: vec![],
            })
            .await;
    }
}
