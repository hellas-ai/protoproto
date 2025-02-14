use crate::{
    automaton::{ModelState, RegisterModel, RunnerBuilder, Timeout},
    models::pure::{
        net::tcp::state::TcpState,
        net::tcp_client::state::TcpClientState,
        net::tcp_server::state::TcpServerState,
        prng::state::{PRNGConfig, PRNGState},
        tests::{
            echo_client::{
                action::EchoClientAction,
                state::{EchoClientConfig, EchoClientState},
            },
            echo_server::{
                action::EchoServerAction,
                state::{EchoServerConfig, EchoServerState},
            },
        },
        time::state::TimeState,
    },
};
use muchin_model_state_derive::ModelState;
use std::any::Any;

#[derive(ModelState, Debug)]
pub struct EchoServer {
    pub time: TimeState,
    pub tcp: TcpState,
    pub tcp_server: TcpServerState,
    pub echo_server: EchoServerState,
}

impl EchoServer {
    pub fn from_config(config: EchoServerConfig) -> Self {
        Self {
            time: TimeState::default(),
            tcp: TcpState::new(),
            tcp_server: TcpServerState::new(),
            echo_server: EchoServerState::from_config(config),
        }
    }
}

#[derive(ModelState, Debug)]
pub struct EchoClient {
    pub prng: PRNGState,
    pub time: TimeState,
    pub tcp: TcpState,
    pub tcp_client: TcpClientState,
    pub echo_client: EchoClientState,
}

impl EchoClient {
    pub fn from_config(config: EchoClientConfig) -> Self {
        Self {
            prng: PRNGState::from_config(PRNGConfig { seed: 1337 }),
            time: TimeState::default(),
            tcp: TcpState::new(),
            tcp_client: TcpClientState::new(),
            echo_client: EchoClientState::from_config(config),
        }
    }
}

#[derive(ModelState, Debug)]
pub enum EchoNetwork {
    EchoServer(EchoServer),
    EchoClient(EchoClient),
}

impl RegisterModel for EchoNetwork {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<EchoClientState>()
            .register::<EchoServerState>()
    }
}

#[test]
fn echo_server_1_client() {
    RunnerBuilder::<EchoNetwork>::new()
        .register::<EchoNetwork>()
        .instance(
            EchoNetwork::EchoServer(EchoServer::from_config(EchoServerConfig {
                address: "127.0.0.1:8888".to_string(),
                max_connections: 1,
                poll_timeout: 100,
                recv_timeout: 500,
            })),
            || EchoServerAction::Tick.into(),
        )
        .instance(
            EchoNetwork::EchoClient(EchoClient::from_config(EchoClientConfig {
                connect_to_address: "127.0.0.1:8888".to_string(),
                connect_timeout: Timeout::Millis(1000),
                poll_timeout: 100,
                max_connection_attempts: 10,
                retry_interval_ms: 500,
                max_send_size: 10240,
                min_rnd_timeout: 1000,
                max_rnd_timeout: 10000,
            })),
            || EchoClientAction::Tick.into(),
        )
        .build()
        .run()
}

fn echo_server_n_clients(n_clients: u64) {
    let mut builder = RunnerBuilder::<EchoNetwork>::new()
        .register::<EchoNetwork>()
        .instance(
            EchoNetwork::EchoServer(EchoServer::from_config(EchoServerConfig {
                address: "127.0.0.1:8888".to_string(),
                max_connections: n_clients as usize,
                poll_timeout: 100 / n_clients,
                recv_timeout: 500 * n_clients,
            })),
            || EchoServerAction::Tick.into(),
        );

    for _ in 0..n_clients {
        builder = builder.instance(
            EchoNetwork::EchoClient(EchoClient::from_config(EchoClientConfig {
                connect_to_address: "127.0.0.1:8888".to_string(),
                connect_timeout: Timeout::Millis(1000 * n_clients),
                poll_timeout: 100 / n_clients,
                max_connection_attempts: 10,
                retry_interval_ms: 500,
                max_send_size: 1024 / n_clients,
                min_rnd_timeout: 1000,
                max_rnd_timeout: 1000 * n_clients,
            })),
            || EchoClientAction::Tick.into(),
        );
    }

    builder.build().record("echo_network")
}

#[test]
fn echo_server_5_clients() {
    echo_server_n_clients(5)
}

#[test]
fn echo_server_50_clients() {
    // WARNING: this test probably needs an increase in the fd limit (ulimit -n 10000)
    echo_server_n_clients(50)
}
