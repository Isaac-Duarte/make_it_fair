use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    response::IntoResponse,
    routing::get,
    Router, ServiceExt,
};
use log::{error, info};
use make_it_fair::{constant, cs2_interface::Player, Cs2Interface, Pid, ProcessHandle};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use tokio::time::Duration;
use tower_http::services::ServeDir;

#[derive(Serialize, Clone)]
struct Payload {
    players: Vec<Player>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();

    let (tx, _) = broadcast::channel::<Payload>(16);
    let tx = Arc::new(tx);

    let tx_clone = tx.clone();

    let process =
        ProcessHandle::from_pid(Pid::from_process_name(constant::PROCESS_NAME).await?).await?;

    let interface = Cs2Interface::new(process)?;

    std::thread::spawn(move || {
        if let Err(e) = cs2_thread(interface, tx_clone) {
            error!("Error in CS2 thread: {:?}", e);
        }
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .nest_service("/", ServeDir::new("web"))
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(tx): State<Arc<broadcast::Sender<Payload>>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!("New WebSocket connection from: {}", addr);

    ws.on_upgrade(move |socket| handle_socket(socket, tx))
}

fn cs2_thread(
    interface: Cs2Interface,
    tx: Arc<broadcast::Sender<Payload>>,
) -> Result<(), anyhow::Error> {

    loop {
        if tx.receiver_count() > 0 {
            let players = interface
                .get_players()?
                .into_iter()
                .filter(|player| player.health > 0)
                .collect();

            if let Err(e) = tx.send(Payload { players }) {
                error!("Failed to send data: {}", e);
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

async fn handle_socket(mut socket: WebSocket, tx: Arc<broadcast::Sender<Payload>>) {
    let mut rx = tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(players) => {
                        if let Err(e) = socket.send(Message::Text(serde_json::to_string(&players).unwrap())).await {
                            error!("Failed to send message: {:?}", e);
                            return;
                        }
                    },
                    Err(e) => {
                        error!("Broadcast error: {:?}", e);
                        return;
                    },
                }
            },
            result = socket.recv() => {
                match result {
                    Some(Ok(_)) => {
                    },
                    Some(Err(e)) => {
                        error!("WebSocket error: {:?}", e);
                        return;
                    },
                    None => {
                        // Client disconnected
                        return;
                    }
                }
            },
        }
    }
}
