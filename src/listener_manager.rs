use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::{Result, Context};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Sender, Receiver};

use crate::listener::{Accepted, Listener};

const MAX_UNACCEPTED_SOCKETS: usize = 100;

pub struct ListenerManager {
    listeners: Mutex<HashMap<SocketAddr, Listener>>,
    socket_tx: Sender<Accepted>,
    socket_rx: Mutex<Receiver<Accepted>>,
}

impl ListenerManager {
    pub fn new() -> Self {
        let (socket_tx, socket_rx) = mpsc::channel(MAX_UNACCEPTED_SOCKETS);
        let socket_rx = Mutex::new(socket_rx);

        Self {
            listeners: Mutex::default(),
            socket_tx,
            socket_rx,
        }
    }

    pub async fn start_listening_on(&self, listen_addr: SocketAddr) -> Result<()> {
        let mut listeners = self.listeners.lock().await;

        if listeners.contains_key(&listen_addr) {
            return Ok(());
        }

        let listener = Listener::start(listen_addr, self.socket_tx.clone()).await
            .context("Failed to start listener")?;

        listeners.insert(listen_addr, listener);
        
        Ok(())
    }

    pub async fn stop_listening_on(&self, addr: SocketAddr) {
        let mut listeners = self.listeners.lock().await;

        if let Some(listener) = listeners.remove(&addr) {
            listener.shutdown().await;
        }
    }

    pub async fn accept(&self) -> Result<Accepted> {
        let mut socket_rx = self.socket_rx.lock().await;

        let accepted = socket_rx.recv().await
            .context("BUG: Listener manager socket_rx dropped")?;

        Ok(accepted)
    }
}
