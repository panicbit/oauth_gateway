use std::net::SocketAddr;

use anyhow::*;
use async_shutdown::Shutdown;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Sender;
use tokio::time::{self, Duration};

pub struct Listener {
    listen_addr: SocketAddr,
    shutdown: Shutdown,
}

impl Listener {
    pub async fn start(listen_addr: SocketAddr, sender: Sender<Accepted>) -> Result<Self> {
        let shutdown = Shutdown::new();
        let this = Self {
            listen_addr,
            shutdown: shutdown.clone(),
        };

        let listener = TcpListener::bind(listen_addr).await
            .with_context(|| format!("Failed to listen on {}", listen_addr))?;

        let listener_loop = async move {
            loop {
                let (stream, remote_addr) = match listener.accept().await.context("Tcp accept failed") {
                    Ok(accepted) => accepted,
                    Err(err) => {
                        eprintln!("{:#}", err);
                        time::sleep(Duration::from_secs(1)).await;
                        continue;
                    },
                };

                let accepted = Accepted {
                    listen_addr,
                    remote_addr,
                    stream,
                };

                // TODO: try to send immediately and limit capacity
                if sender.send(accepted).await.is_err() {
                    break;
                }
            }
        };
        let listener_loop = shutdown.wrap_cancel(listener_loop);
        let listener_loop = shutdown.wrap_wait(listener_loop)?;

        tokio::spawn(listener_loop);

        Ok(this)
    }

    pub async fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub async fn shutdown(&self) {
        self.shutdown.shutdown();
        self.shutdown.wait_shutdown_complete().await;
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown.shutdown();
    }
}

pub struct Accepted {
    pub listen_addr: SocketAddr,
    pub remote_addr: SocketAddr,
    pub stream: TcpStream,
}
