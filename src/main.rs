use std::io::BufRead;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use tokio::task;

#[derive(Debug, Clone)]
struct Server(String);
#[derive(Debug, Clone)]
struct Port(u16);
#[derive(Debug, Clone)]
struct Nickname(String);
#[derive(Debug, Clone)]
struct Channel(String);

#[derive(Debug, Clone)]
enum Message {
    Raw(String),
    Join(Channel),
    Pong(String),
}

async fn read_input(tx: Sender<Message>) {
    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);
    loop {
        let mut buffer = String::new();
        match stdin_reader.read_line(&mut buffer).await {
            Ok(_) => {
                let trimmed_buffer = buffer.trim();
                let message = if trimmed_buffer.starts_with("JOIN ") {
                    Message::Join(Channel(trimmed_buffer[5..].trim().to_string()))
                } else {
                    Message::Raw(trimmed_buffer.to_string())
                };
                tx.send(message).await.unwrap();
            }
            Err(_) => break,
        }
    }
}
async fn read_stream<R: AsyncBufRead + Unpin>(mut stream: R, tx: Sender<Message>) {
    loop {
        let mut buffer = String::new();
        match stream.read_line(&mut buffer).await {
            Ok(_) => {
                println!("Received line from server: {}", buffer); // Add this line for logging
                let trimmed_buffer = buffer.trim();
                let message = if trimmed_buffer.starts_with("PING") {
                    Message::Pong(trimmed_buffer[5..].trim().to_string())
                } else {
                    Message::Raw(trimmed_buffer.to_string())
                };
                tx.send(message).await.unwrap();
            }
            Err(_) => break,
        }
    }
}

async fn write_stream<W: AsyncWrite + Unpin>(mut stream: W, mut rx: Receiver<Message>) {
    while let Some(message) = rx.recv().await {
        match message {
            Message::Join(channel) => {
                let join_message = format!("JOIN {}\r\n", channel.0);
                println!("Sending join message: {}", join_message); // Add this line for logging
                stream.write_all(join_message.as_bytes()).await.unwrap();
            }
            Message::Raw(raw_message) => {
                let raw_message = format!("{}\r\n", raw_message);
                println!("Sending raw message: {}", raw_message); // Add this line for logging
                stream.write_all(raw_message.as_bytes()).await.unwrap();
            }
            Message::Pong(server) => {
                let pong_message = format!("PONG {}\r\n", server);
                println!("Sending pong message: {}", pong_message); // Add this line for logging
                stream.write_all(pong_message.as_bytes()).await.unwrap();
            }
        }
    }
}
#[tokio::main]
async fn main() {
    let (_tx_input, _rx_input) = channel::<Message>(32);
    let (tx_stream, rx_stream) = channel::<Message>(32);
    let tx_stream_read = tx_stream.clone();
    let tx_stream_input = tx_stream.clone();
    println!("Enter server address:");
    let server = read_line();

    println!("Enter server port (default: 6667):");
    let port = read_line().parse::<u16>().unwrap_or(6667);

    println!("Enter your nickname:");
    let nickname = read_line();

    let stream = TcpStream::connect(format!("{}:{}", server, port))
        .await
        .unwrap();

    let (stream_reader, mut stream_writer) = stream.into_split();
    stream_writer
        .write_all(format!("NICK {}\r\n", nickname).as_bytes())
        .await
        .unwrap();
    stream_writer
        .write_all(format!("USER {} 0 * :{}\r\n", nickname, nickname).as_bytes())
        .await
        .unwrap();
    stream_writer.write_all(b"CAP REQ :sasl\r\n").await.unwrap();
    let read_input_task = task::spawn(read_input(tx_stream_input));
    let read_stream_task = task::spawn(read_stream(BufReader::new(stream_reader), tx_stream_read));
    let write_stream_task = task::spawn(write_stream(BufWriter::new(stream_writer), rx_stream));
    tokio::try_join!(read_input_task, read_stream_task, write_stream_task).unwrap();
}
fn read_line() -> String {
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut stdin_lock = stdin.lock();
    stdin_lock.read_line(&mut input).unwrap();
    input.trim().to_string()
}
