use chatserver::{client, server};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum RunArguments {
    Server {},
    Client { username: Option<String> },
}

#[tokio::main]
async fn main() {
    match RunArguments::from_args() {
        RunArguments::Server {} => {
            env_logger::init();
            match server::run_server().await {
                Ok(_) => {}
                Err(err) => eprintln!("{:?}", err),
            }
        }
        RunArguments::Client { username } => {
            match client::run_client(username.as_ref().map(|s| &**s)).await {
                Ok(_) => {}
                Err(err) => eprintln!("{:?}", err),
            }
        }
    }
}
