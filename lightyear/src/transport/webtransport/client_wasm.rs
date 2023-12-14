#![cfg(target_family = "wasm")]
//! WebTransport client implementation.
use super::MTU;
use crate::transport::{PacketReceiver, PacketSender, Transport};
use anyhow::Context;
use bevy::tasks::{IoTaskPool, TaskPool};
use futures_lite::future;
use std::net::SocketAddr;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{debug, error, info};
use web_sys::js_sys::{Array, Uint8Array};
use web_sys::wasm_bindgen::JsValue;
use web_sys::WebTransportHash;

use base64::prelude::{Engine as _, BASE64_STANDARD};

use xwt::current::{Connection, Endpoint};
use xwt_core::prelude::*;

/// WebTransport client socket
pub struct WebTransportClientSocket {
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

impl WebTransportClientSocket {
    pub fn new(client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
        Self {
            client_addr,
            server_addr,
        }
    }
}

fn js_array(values: &[&str]) -> JsValue {
    return JsValue::from(
        values
            .into_iter()
            .map(|x| JsValue::from_str(x))
            .collect::<Array>(),
    );
}

impl Transport for WebTransportClientSocket {
    fn local_addr(&self) -> SocketAddr {
        self.client_addr
    }

    fn listen(&mut self) -> anyhow::Result<(Box<dyn PacketSender>, Box<dyn PacketReceiver>)> {
        let client_addr = self.client_addr;
        let server_addr = self.server_addr;
        let (to_server_sender, mut to_server_receiver) = mpsc::unbounded_channel();
        let (from_server_sender, from_server_receiver) = mpsc::unbounded_channel();

        let server_url = format!("https://{}", server_addr);
        debug!(
            "Starting client webtransport task with server url: {}",
            &server_url
        );

        let mut options = web_sys::WebTransportOptions::new();
        let hashes = Array::new();
        // cert_hash = base
        let base64_hashes = ["8GLWBL0MAlhPcZ4RbFSn7oULnIOAG6bQak3pf5yqx48="];
        let decoded_hashes = base64_hashes
            .iter()
            .map(|x| BASE64_STANDARD.decode(x).unwrap())
            .collect::<Vec<_>>();
        for hash in decoded_hashes {
            let digest = Uint8Array::from(hash.as_slice());

            let mut jshash = WebTransportHash::new();
            jshash.algorithm("sha-256").value(&digest);

            hashes.push(&jshash);
        }
        options.server_certificate_hashes(&hashes);
        let endpoint = xwt::web_sys::Endpoint { options };

        IoTaskPool::get().spawn(async move {
            // convert the endpoint from wtransport/web_sys to xwt
            // let endpoint = xwt::current::Endpoint(endpoint);

            let connecting = endpoint
                .connect(&server_url)
                .await
                .map_err(|e| {
                    error!("failed to connect to server: {:?}", e);
                })
                .unwrap();
            let connection = connecting
                .wait_connect()
                .await
                .map_err(|e| {
                    error!("failed to connect to server: {:?}", e);
                })
                .unwrap();
            loop {
                tokio::select! {
                    // receive messages from server
                    x = connection.receive_datagram() => {
                        match x {
                            Ok(data) => {
                                from_server_sender.send(data).unwrap();
                            }
                            Err(e) => {
                                error!("receive_datagram error: {:?}", e);
                            }
                        }
                    }

                    // send messages to server
                    Some(msg) = to_server_receiver.recv() => {
                        connection.send_datagram(msg).await.unwrap_or_else(|e| {
                            error!("send_datagram error: {:?}", e);
                        });
                    }
                }
            }
            // loop {
            //     let receive = async move {
            //         let x = connection.receive_datagram().await;
            //         match x {
            //             Ok(data) => {
            //                 from_server_sender.send(data).unwrap();
            //             }
            //             Err(e) => {
            //                 error!("receive_datagram error: {:?}", e);
            //             }
            //         }
            //     };
            //     let send = async move {
            //         // send messages to server
            //         if let Some(msg) = to_server_receiver.recv().await {
            //             connection.send_datagram(msg).await.unwrap_or_else(|e| {
            //                 error!("send_datagram error: {:?}", e);
            //             });
            //         }
            //     };
            //     future::race(receive, send);
            // }
        });
        let packet_sender = WebTransportClientPacketSender { to_server_sender };
        let packet_receiver = WebTransportClientPacketReceiver {
            server_addr,
            from_server_receiver,
            buffer: [0; MTU],
        };
        Ok((Box::new(packet_sender), Box::new(packet_receiver)))
    }
}

struct WebTransportClientPacketSender {
    to_server_sender: mpsc::UnboundedSender<Box<[u8]>>,
}

impl PacketSender for WebTransportClientPacketSender {
    fn send(&mut self, payload: &[u8], address: &SocketAddr) -> std::io::Result<()> {
        let data = payload.to_vec().into_boxed_slice();
        self.to_server_sender
            .send(data)
            .map_err(|e| std::io::Error::other(format!("send_datagram error: {:?}", e)))
    }
}

struct WebTransportClientPacketReceiver {
    server_addr: SocketAddr,
    from_server_receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    buffer: [u8; MTU],
}

impl PacketReceiver for WebTransportClientPacketReceiver {
    fn recv(&mut self) -> std::io::Result<Option<(&mut [u8], SocketAddr)>> {
        match self.from_server_receiver.try_recv() {
            Ok(datagram) => {
                // convert from datagram to payload via xwt
                let data = datagram.as_slice();
                self.buffer[..data.len()].copy_from_slice(data);
                Ok(Some((&mut self.buffer[..data.len()], self.server_addr)))
            }
            Err(e) => {
                if e == TryRecvError::Empty {
                    Ok(None)
                } else {
                    Err(std::io::Error::other(format!(
                        "receive_datagram error: {:?}",
                        e
                    )))
                }
            }
        }
    }
}
