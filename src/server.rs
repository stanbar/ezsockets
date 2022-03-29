use crate::Error;
use crate::Session;
use crate::SessionExt;
use crate::Socket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use tokio::sync::mpsc;

struct ServerActor<E: ServerExt> {
    connections: mpsc::UnboundedReceiver<(Socket, SocketAddr, E::Args)>,
    calls: mpsc::UnboundedReceiver<E::Params>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(&mut self) -> Result<(), Error> {
        tracing::info!("starting server");
        loop {
            tokio::select! {
                Some((socket, address, args)) = self.connections.recv() => {
                    self.extension.accept(socket, address, args).await?;
                    tracing::info!("connection from {address} accepted");
                }
                Some(params) = self.calls.recv() => {
                    self.extension.call(params).await?
                }
                else => break
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait ServerExt: Send {
    type Session: SessionExt;
    type Params: Send + std::fmt::Debug;
    type Args: std::fmt::Debug;

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        args: Self::Args,
    ) -> Result<Session<<Self::Session as SessionExt>::Params>, Error>;
    async fn disconnected(&mut self, id: <Self::Session as SessionExt>::ID)
        -> Result<(), Error>;
    async fn call(&mut self, params: Self::Params) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Server<P: std::fmt::Debug = (), A: std::fmt::Debug = ()> {
    connections: mpsc::UnboundedSender<(Socket, SocketAddr, A)>,
    calls: mpsc::UnboundedSender<P>,
}

impl<P: std::fmt::Debug, A: std::fmt::Debug> From<Server<P, A>> for mpsc::UnboundedSender<P> {
    fn from(server: Server<P, A>) -> Self {
        server.calls
    }
}

impl<P: std::fmt::Debug + Send, A: std::fmt::Debug + Send> Server<P, A> {
    pub fn create<E: ServerExt<Params = P, Args = A> + 'static>(
        create: impl FnOnce(Self) -> E,
    ) -> (Self, impl Future<Output = Result<(), Error>>) {
        let (connection_sender, connection_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let handle = Self {
            connections: connection_sender,
            calls: call_sender,
        };
        let extension = create(handle.clone());
        let mut actor = ServerActor {
            connections: connection_receiver,
            calls: call_receiver,
            extension,
        };
        let future = tokio::spawn(async move {
            actor.run().await?;
            Ok::<_, Error>(())
        });
        let future = async move { future.await.unwrap() };
        (handle, future)
    }
}

impl<P: std::fmt::Debug, A: std::fmt::Debug> Server<P, A> {
    pub async fn accept(&self, socket: Socket, address: SocketAddr, args: A) {
        self.connections
            .send((socket, address, args))
            .map_err(|_| ())
            .unwrap();
    }

    pub async fn call(&self, params: P) {
        self.calls.send(params).map_err(|_| ()).unwrap();
    }
}

impl<P: std::fmt::Debug, A: std::fmt::Debug> std::clone::Clone for Server<P, A> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            calls: self.calls.clone(),
        }
    }
}
