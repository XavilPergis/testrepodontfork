use structopt::StructOpt;
use tokio::{io::AsyncWriteExt, net::TcpStream};

pub mod error;
pub mod server;

#[derive(Debug, StructOpt)]
pub enum RunArguments {
    Server {},
    Client {},
}

async fn run_client() -> error::ClientResult<()> {
    let mut stream = TcpStream::connect("127.0.0.1:8080").await?;

    let mut buffer = Vec::new();
    let mut serializer = server::PacketSerializerContext::new(&mut buffer);

    serializer.serialize(&server::ClientMessage::Connect);
    serializer.serialize(&server::ClientMessage::Message("hello!".into()));
    serializer.serialize(&server::ClientMessage::Disconnect);
    stream.write_all(&mut buffer).await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    match RunArguments::from_args() {
        RunArguments::Server {} => match server::run_server().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        },
        RunArguments::Client {} => match run_client().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        },
    }
}
