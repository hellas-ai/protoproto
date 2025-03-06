use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Duration;
use warp::{Filter, Reply};
use tokio::sync::mpsc;
use tokio::time::interval;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use uuid::Uuid;

use hellas_morpheus::{
    MorpheusHarness, ProcessId, Message, Block, BlockId, BlockType,
    QuorumCertificate, Vote, ViewMessage, EndViewMessage
};

// Serialize/deserialize friendly versions of our types
#[derive(Serialize)]
struct ApiProcessId {
    id: usize,
}

#[derive(Serialize)]
struct ApiBlockId {
    block_type: String,
    author: usize,
    view: usize,
    slot: usize,
}

#[derive(Serialize)]
struct ApiBlock {
    id: ApiBlockId,
    height: usize,
    prev_qcs: Vec<ApiQcId>,
    one_qc: Option<ApiQcId>,
    justification: Vec<(usize, usize)>, // (view, sender)
}

#[derive(Serialize)]
struct ApiQcId {
    block_id: ApiBlockId,
}

#[derive(Serialize)]
struct ApiQuorumCertificate {
    id: ApiQcId,
    height: usize,
}

#[derive(Serialize)]
struct ApiVote {
    vote_num: usize,
    block_id: ApiBlockId,
    voter: usize,
}

#[derive(Serialize)]
struct ApiViewMessage {
    view: usize,
    qc_id: Option<ApiQcId>,
    sender: usize,
}

#[derive(Serialize)]
struct ApiEndViewMessage {
    view: usize,
    sender: usize,
}

#[derive(Serialize)]
enum ApiMessage {
    Block(ApiBlock),
    Vote(ApiVote),
    QC(ApiQuorumCertificate),
    ViewMsg(ApiViewMessage),
    EndViewMsg(ApiEndViewMessage),
}

#[derive(Serialize)]
struct ApiMessageHistory {
    from: usize,
    to: usize,
    message: ApiMessage,
}

#[derive(Serialize)]
struct ApiProcessState {
    id: usize,
    view: usize,
    phase: usize,
    is_leader: bool,
}

#[derive(Serialize)]
struct ApiSimulationState {
    processes: Vec<ApiProcessState>,
    message_history: Vec<ApiMessageHistory>,
    blocks: Vec<ApiBlock>,
}

// Shared state for our simulation
struct AppState {
    harness: MorpheusHarness,
    clients: HashMap<String, mpsc::UnboundedSender<Result<warp::ws::Message, warp::Error>>>,
}

// Convert our Rust types to API-friendly types
fn convert_process_id(pid: ProcessId) -> ApiProcessId {
    ApiProcessId { id: pid.0 }
}

fn convert_block_type(block_type: BlockType) -> String {
    match block_type {
        BlockType::Genesis => "Genesis".to_string(),
        BlockType::Lead => "Lead".to_string(),
        BlockType::Tr => "Tr".to_string(),
    }
}

fn convert_block_id(block_id: BlockId) -> ApiBlockId {
    ApiBlockId {
        block_type: convert_block_type(block_id.block_type),
        author: block_id.auth.0,
        view: block_id.view,
        slot: block_id.slot,
    }
}

fn convert_qc_id(qc_id: hellas_morpheus::QcId) -> ApiQcId {
    ApiQcId {
        block_id: convert_block_id(qc_id.block_id),
    }
}

fn convert_block(block: Block) -> ApiBlock {
    ApiBlock {
        id: convert_block_id(block.id),
        height: block.height,
        prev_qcs: block.prev_qcs.into_iter().map(convert_qc_id).collect(),
        one_qc: block.one_qc.map(convert_qc_id),
        justification: block.justification.into_iter()
            .map(|(view, pid)| (view, pid.0))
            .collect(),
    }
}

fn convert_qc(qc: QuorumCertificate) -> ApiQuorumCertificate {
    ApiQuorumCertificate {
        id: convert_qc_id(qc.id),
        height: qc.height,
    }
}

fn convert_vote(vote: Vote) -> ApiVote {
    ApiVote {
        vote_num: vote.vote_num,
        block_id: convert_block_id(vote.block_id),
        voter: vote.voter.0,
    }
}

fn convert_view_message(vm: ViewMessage) -> ApiViewMessage {
    ApiViewMessage {
        view: vm.view,
        qc_id: vm.qc_id.map(convert_qc_id),
        sender: vm.sender.0,
    }
}

fn convert_end_view_message(evm: EndViewMessage) -> ApiEndViewMessage {
    ApiEndViewMessage {
        view: evm.view,
        sender: evm.sender.0,
    }
}

fn convert_message(message: Message) -> ApiMessage {
    match message {
        Message::Block(block) => ApiMessage::Block(convert_block(block)),
        Message::Vote(vote) => ApiMessage::Vote(convert_vote(vote)),
        Message::QC(qc) => ApiMessage::QC(convert_qc(qc)),
        Message::ViewMsg(vm) => ApiMessage::ViewMsg(convert_view_message(vm)),
        Message::EndViewMsg(evm) => ApiMessage::EndViewMsg(convert_end_view_message(evm)),
    }
}

fn convert_message_history(
    history: &[(ProcessId, ProcessId, Message)]
) -> Vec<ApiMessageHistory> {
    history.iter().map(|(from, to, message)| {
        ApiMessageHistory {
            from: from.0,
            to: to.0,
            message: convert_message(message.clone()),
        }
    }).collect()
}

// Get the current simulation state
fn get_simulation_state(harness: &MorpheusHarness) -> ApiSimulationState {
    let mut processes = Vec::new();
    
    // Get process count
    let process_count = (0..100)
        .map(ProcessId)
        .filter(|pid| harness.get_process(*pid).is_some())
        .count();
    
    // Get all process states
    for i in 0..process_count {
        let pid = ProcessId(i);
        if let Some(process) = harness.get_process(pid) {
            let phase = *process.phase_i.get(&process.view_i).unwrap_or(&0);
            let is_leader = process.id == process.lead(process.view_i);
            
            processes.push(ApiProcessState {
                id: i,
                view: process.view_i,
                phase,
                is_leader,
            });
        }
    }
    
    // Get message history
    let message_history = convert_message_history(harness.get_message_history());
    
    // Get all blocks from all processes
    let mut blocks = Vec::new();
    for i in 0..process_count {
        let pid = ProcessId(i);
        if let Some(process) = harness.get_process(pid) {
            for (_, block) in process.blocks.iter() {
                blocks.push(convert_block(block.clone()));
            }
        }
    }
    
    // Remove duplicates from blocks (keeping the first instance)
    blocks.sort_by(|a, b| a.height.cmp(&b.height));
    blocks.dedup_by(|a, b| {
        a.id.block_type == b.id.block_type && 
        a.id.author == b.id.author && 
        a.id.view == b.id.view && 
        a.id.slot == b.id.slot
    });
    
    ApiSimulationState {
        processes,
        message_history,
        blocks,
    }
}

#[tokio::main]
async fn main() {
    // Create our simulation state
    let n = 4; // Number of nodes
    let f = 1; // Maximum number of faulty nodes
    
    let harness = MorpheusHarness::new(n, f);
    let state = Arc::new(Mutex::new(AppState {
        harness,
        clients: HashMap::new(),
    }));
    
    // Set up routes
    
    // Serve static files
    let static_files = warp::path("static").and(warp::fs::dir("./frontend/build/static"));
    let index = warp::path::end().and(warp::fs::file("./frontend/build/index.html"));
    let favicon = warp::path("favicon.ico").and(warp::fs::file("./frontend/build/favicon.ico"));
    
    // API routes
    let state_clone = state.clone();
    let api_state = warp::path("api")
        .and(warp::path("state"))
        .and(warp::get())
        .map(move || {
            let state = state_clone.lock().unwrap();
            let simulation_state = get_simulation_state(&state.harness);
            warp::reply::json(&simulation_state)
        });
    
    // WebSocket route
    let state_clone = state.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let state = state_clone.clone();
            ws.on_upgrade(move |socket| handle_ws_client(socket, state))
        });
    
    // Combine routes
    let routes = static_files
        .or(index)
        .or(favicon)
        .or(api_state)
        .or(ws_route);
    
    // Create a background task to advance the simulation
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(1000));
        
        loop {
            interval.tick().await;
            
            let mut state = state_clone.lock().unwrap();
            let steps = state.harness.run_steps(1);
            
            if steps > 0 {
                // Get current state and broadcast to all clients
                let simulation_state = get_simulation_state(&state.harness);
                let json = serde_json::to_string(&simulation_state).unwrap();
                
                // Send to all connected clients
                let mut disconnected_clients = Vec::new();
                for (client_id, tx) in &state.clients {
                    if let Err(_) = tx.send(Ok(warp::ws::Message::text(json.clone()))) {
                        disconnected_clients.push(client_id.clone());
                    }
                }
                
                // Clean up disconnected clients
                for client_id in disconnected_clients {
                    state.clients.remove(&client_id);
                }
            }
        }
    });
    
    println!("Starting server at http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn handle_ws_client(
    websocket: warp::ws::WebSocket,
    state: Arc<Mutex<AppState>>,
) {
    println!("New WebSocket connection");
    
    // Split the socket
    let (mut ws_tx, mut ws_rx) = websocket.split();
    
    // Create a channel for sending messages to the client
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // Convert messages from rx to ws_tx and forward them
    tokio::task::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Ok(msg) = message {
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Generate a client ID
    let client_id = Uuid::new_v4().to_string();
    
    // Store the sender in our clients map
    {
        let mut state = state.lock().unwrap();
        state.clients.insert(client_id.clone(), tx.clone());
        
        // Send the initial state
        let simulation_state = get_simulation_state(&state.harness);
        let json = serde_json::to_string(&simulation_state).unwrap();
        let _ = tx.send(Ok(warp::ws::Message::text(json)));
    }
    
    // Process incoming messages (we don't expect any, but handle disconnection)
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(_) => {
                // We're not expecting client messages, but you could handle them here
            }
            Err(_) => {
                // Client disconnected or errored
                let mut state = state.lock().unwrap();
                state.clients.remove(&client_id);
                break;
            }
        }
    }
}
