use structopt::StructOpt;

pub mod client;
pub mod common;
pub mod server;

#[derive(Debug, StructOpt)]
pub enum RunArguments {
    Server {},
    Client {},
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
        RunArguments::Client {} => match client::run_client().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        },
    }
}
