use argh::FromArgs;

#[derive(FromArgs, PartialEq, Debug)]
/// Top-level command.
struct TopLevel {
    #[argh(subcommand)]
    nested: Subcommands,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum Subcommands {
    RunDaemon(RunDaemon),
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run a fresh daemon in the background
#[argh(subcommand, name = "run-daemon")]
struct RunDaemon {
    #[argh(option)]
    /// hex-encoded private key
    privkey: String,
    #[argh(option, default = "17271")]
    /// libp2p port (default 17271)
    port: u16,
    #[argh(option, default = "17272")]
    /// listen port for the webui (default none)
    webui_listen: u16,

}

fn main() {
    let whats_up: TopLevel = argh::from_env();

    println!("{:?}", whats_up);
}
