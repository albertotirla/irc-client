use futures::prelude::*;
use irc::client::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::stdin,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use toml;
#[derive(Debug, Deserialize, Serialize)]
struct AppConfig {
    nickname: String,
    username: Option<String>,
    realname: Option<String>,
    server: String,
    port: Option<u16>,
    use_tls: Option<bool>,
    channels: Vec<String>,
}

async fn read_config() -> AppConfig {
    if Path::new("config.toml").exists() {
        println!("found configuration file");
        let config_str = fs::read_to_string("config.toml").expect("Unable to read config.toml");
        toml::from_str(&config_str).expect("Unable to parse config.toml")
    } else {
        println!("configuration file not found!");
        println!("In the following prompts, you'll be asked to fill in the required information about yourself and your irc network, in order to connect to your server");
        let mut use_tls = String::new();
        let mut nickname = String::new();
        let mut server = String::new();
        let mut port = String::new();
        let mut channels = String::new();

        println!("Type your nickname, then press enter: ");
        stdin().read_line(&mut nickname).unwrap();
        println!("Type the address of your irc network. This is the domain one connects to specifically with an irc client, for example `irc.libera.chat`: ");
        stdin().read_line(&mut server).unwrap();
        println!("enter the port for {} (default: 6667):", &server);
        stdin().read_line(&mut port).unwrap();
        println!("Use TLS? (y/n): ");
        stdin().read_line(&mut use_tls).unwrap();
        println!("Optionally, type in a list of channels you want to be prejoined to on startup, comma sepparated");
        stdin().read_line(&mut channels).unwrap();
        println!("configuration complete!");
        let channels = channels
            .trim()
            .split(',')
            .map(|channel| channel.trim().to_string())
            .collect();
        let port = port.parse::<u16>().unwrap_or(6667);
        let use_tls = use_tls.trim().to_lowercase() == "y";
        let config = AppConfig {
            nickname: nickname.trim().to_string(),
            username: None,
            realname: None,
            server: server.trim().to_string(),
            port: Some(port),
            use_tls: Some(use_tls),
            channels,
        };

        let config_str = toml::to_string(&config).expect("Unable to serialize config");
        fs::write("config.toml", config_str).expect("Unable to write config.toml");

        config
    }
}
#[derive(Debug)]
enum UserCommand {
    Join(String),
    Msg(String),
    Switch(String),
    Unknown,
}

async fn parse_user_input(line: &str) -> UserCommand {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return UserCommand::Unknown;
    }

    match parts[0] {
        "/join" if parts.len() > 1 => UserCommand::Join(parts[1].to_owned()),
        "/msg" if parts.len() > 1 => {
            let message = parts[1..].join(" ");
            UserCommand::Msg(message)
        }
        "/switch" if parts.len() > 1 => UserCommand::Switch(parts[1].to_owned()),
        _ => UserCommand::Unknown,
    }
}
async fn handle_user_input(sender: mpsc::Sender<UserCommand>) {
    use tokio::io::{stdin};
    use tokio_util::codec::{FramedRead, LinesCodec};
    let stdin = stdin();
    let mut lines = FramedRead::new(stdin, LinesCodec::new());
    while let Some(line) = lines.next().await {
        if let Ok(input) = line {
            let cmd = parse_user_input(&input).await;
            println!("{:?}", cmd);
            if let Err(_) = sender.send(cmd).await {
                println!("error sending message");
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = read_config().await;

    let irc_config = Config {
        nickname: Some(config.nickname),
        username: config.username,
        realname: config.realname,
        server: Some(config.server),
        port: config.port,
        use_tls: config.use_tls,
        channels: config.channels.clone(),
        ..Config::default()
    };

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;
    let (tx, mut rx) = mpsc::channel::<UserCommand>(32);
    let joined_channels = Arc::new(Mutex::new(HashSet::new()));
    let current_channel = Arc::new(Mutex::new(String::new()));
    let mut stream = client.stream()?;
    for channel in &config.channels {
        joined_channels.lock().unwrap().insert(channel.clone());
        *current_channel.lock().unwrap() = channel.clone();
        client.send_join(channel)?;
    }
    let _input_processor = tokio::spawn(handle_user_input(tx));
    while let Some(message) = stream.next().await.transpose()? {
        print!("{}", message);
        match message.command {
            Command::PRIVMSG(ref target, ref msg) => {
                println!("{}: {}", target, msg);
            }
            _ => {}
        }
    }
    let _server_messages_processor = tokio::spawn(async move {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                UserCommand::Join(channel) => {
                    println!("joining channel {}", channel);
                    joined_channels.lock().unwrap().insert(channel.clone());
                    *current_channel.lock().unwrap() = channel.clone();
                    client.send_join(&channel).unwrap();
                }
                UserCommand::Msg(message) => {
                    let target = current_channel.lock().unwrap().clone();
                    if !target.is_empty() {
                        client.send_privmsg(&target, &message).unwrap();
                    } else {
                        println!("No channel currently selected.");
                    }
                }
                UserCommand::Switch(channel) => {
                    if joined_channels.lock().unwrap().contains(&channel) {
                        *current_channel.lock().unwrap() = channel;
                    } else {
                        joined_channels.lock().unwrap().insert(channel.clone());
                        *current_channel.lock().unwrap() = channel.clone();
                        client.send_join(&channel).unwrap();
                    }
                }
                UserCommand::Unknown => {
                    println!("Unknown command or invalid usage.");
                }
            }
        }
    });
    //    tokio::try_join!(input_processor, server_messages_processor)?;
    Ok(())
}
