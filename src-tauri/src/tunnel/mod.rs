use crate::{db::TunnelRule, ssh};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct TunnelHandle {
    pub server_id: String,
    cancel: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl TunnelHandle {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

pub async fn start(
    config: crate::db::ServerConnectionConfig,
    rule: TunnelRule,
) -> Result<TunnelHandle, String> {
    let local_port =
        u16::try_from(rule.local_port).map_err(|_| "Invalid local tunnel port".to_string())?;
    let remote_port =
        u32::try_from(rule.remote_port).map_err(|_| "Invalid remote tunnel port".to_string())?;
    let listener = TcpListener::bind((rule.local_host.as_str(), local_port))
        .await
        .map_err(|error| format!("无法监听 {}:{}：{error}", rule.local_host, rule.local_port))?;
    let handle = ssh::connect(&config).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    let task_cancel = cancel.clone();
    let alive = Arc::new(AtomicBool::new(true));
    let task_alive = alive.clone();
    let server_id = rule.server_id.clone();

    tokio::spawn(async move {
        let connections = Arc::new(Semaphore::new(64));
        while !task_cancel.load(Ordering::Relaxed) && !handle.is_closed() {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                accepted = listener.accept() => {
                    let Ok((socket, origin)) = accepted else { break };
                    let Ok(permit) = connections.clone().try_acquire_owned() else { continue };
                    let channel = match tokio::time::timeout(
                        Duration::from_secs(15),
                        handle.channel_open_direct_tcpip(
                            rule.remote_host.clone(),
                            remote_port,
                            origin.ip().to_string(),
                            u32::from(origin.port()),
                        ),
                    )
                    .await
                    {
                        Ok(Ok(channel)) => channel,
                        Err(_) => continue,
                        Ok(Err(_)) => continue,
                    };
                    tokio::spawn(async move {
                        let _permit = permit;
                        let mut socket = socket;
                        let mut stream = channel.into_stream();
                        let _ = copy_bidirectional(&mut socket, &mut stream).await;
                        let _ = socket.shutdown().await;
                        let _ = stream.shutdown().await;
                    });
                }
            }
        }
        ssh::disconnect(&handle).await;
        task_alive.store(false, Ordering::Relaxed);
    });

    Ok(TunnelHandle {
        server_id,
        cancel,
        alive,
    })
}
