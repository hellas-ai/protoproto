use argh::FromArgs;

#[derive(FromArgs, PartialEq, Debug)]
/// Top-level command.
pub struct TopLevel {
    #[argh(subcommand)]
    pub nested: Subcommands,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
pub enum Subcommands {
    RunDaemon(RunDaemon),
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run a fresh daemon in the background
#[argh(subcommand, name = "run-daemon")]
pub struct RunDaemon {
    #[argh(option)]
    /// hex-encoded private key
    pub privkey: String,
    #[argh(option, default = "17271")]
    /// libp2p port (default 17271)
    pub port: u16,
    #[argh(option, default = "17272")]
    /// listen port for the webui (default none)
    pub webui_listen: u16,
}
