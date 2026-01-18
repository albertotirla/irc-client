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

fn read_config() -> AppConfig {
    if Path::new("config.toml").exists() {
        println!("found configuration file");
        let config_str = fs::read_to_string("config.toml").expect("Unable to read config.toml");
        toml::from_str(&config_str).expect("Unable to parse config.toml")
    } else {
        println!("configuration file not found!");
        println!(
            "In the following prompts, you'll be asked to fill in the required information about yourself and your irc network, in order to connect to your server"
        );
        let mut use_tls = String::new();
        let mut nickname = String::new();
        let mut server = String::new();
        let mut port = String::new();
        let mut channels = String::new();

        println!("Type your nickname, then press enter: ");
        stdin().read_line(&mut nickname).unwrap();
        println!(
            "Type the address of your irc network. This is the domain one connects to specifically with an irc client, for example `irc.libera.chat`: "
        );
        stdin().read_line(&mut server).unwrap();
        println!("enter the port for {} (default: 6667):", &server);
        stdin().read_line(&mut port).unwrap();
        println!("Use TLS? (y/n): ");
        stdin().read_line(&mut use_tls).unwrap();
        println!(
            "Optionally, type in a list of channels you want to be prejoined to on startup, comma sepparated"
        );
        stdin().read_line(&mut channels).unwrap();
        println!("configuration complete!");
        let channels = channels
            .trim()
            .split(',')
            .map(|channel| channel.trim().to_string())
            .collect();
        let port = port.trim_end().parse::<u16>().unwrap_or(6667);
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
    Query(String),
    Unknown,
}

fn parse_user_input(line: &str) -> UserCommand {
    if !line.starts_with("/") {
        return UserCommand::Msg(line.trim_end().into());
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return UserCommand::Unknown;
    }

    match parts[0] {
        "/join" if parts.len() > 1 => UserCommand::Join(parts[1].to_owned()),
        "/query" if parts.len() > 1 => UserCommand::Query(parts[1].to_owned()),
        "/msg" if parts.len() > 1 => {
            let message = parts[1..].join(" ");
            UserCommand::Msg(message)
        }
        "/switch" if parts.len() > 1 => UserCommand::Switch(parts[1].to_owned()),
        _ => UserCommand::Unknown,
    }
}
fn handle_user_input(sender: mpsc::Sender<UserCommand>) {
    loop {
        let stdin = stdin();
        let mut line = String::new();
        stdin.read_line(&mut line).unwrap();
        let cmd = parse_user_input(&line);
        println!("{:?}", cmd);
        line.clear();
        if sender.blocking_send(cmd).is_err() {
            break;
        }
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = read_config();

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
    let input_processor = tokio::task::spawn_blocking(|| handle_user_input(tx));

    let current_channel_clone = current_channel.clone();
    let joined_channels_clone = joined_channels.clone();
    let server_messages_processor = tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                UserCommand::Join(channel) => {
                    println!("joining channel {}", channel);
                    joined_channels_clone
                        .lock()
                        .unwrap()
                        .insert(channel.clone());
                    *current_channel_clone.lock().unwrap() = channel.clone();
                    client.send_join(&channel).unwrap();
                }

                UserCommand::Query(person) => {
                    println!("opening private message with {}", person);
                    joined_channels_clone.lock().unwrap().insert(person.clone());
                    *current_channel_clone.lock().unwrap() = person.clone();
                    client.send_privmsg(&person, "").unwrap();
                }

                UserCommand::Msg(message) => {
                    let target = current_channel_clone.lock().unwrap().clone();
                    if !target.is_empty() {
                        client.send_privmsg(&target, &message).unwrap();
                    } else {
                        println!("No channel currently selected.");
                    }
                }
                UserCommand::Switch(channel) => {
                    if joined_channels_clone.lock().unwrap().contains(&channel) {
                        *current_channel_clone.lock().unwrap() = channel;
                    } else {
                        joined_channels_clone
                            .lock()
                            .unwrap()
                            .insert(channel.clone());
                        *current_channel_clone.lock().unwrap() = channel.clone();
                        client.send_join(&channel).unwrap();
                    }
                }
                UserCommand::Unknown => {
                    println!("Unknown command or invalid usage.");
                }
            }
        }
    });
    while let Some(message) = stream.next().await.transpose()? {
        println!("{}", message);
        if let Command::PRIVMSG(ref target, ref msg) = message.command {
            let target = if let Some(query_target) = message.response_target() {
                query_target
            } else {
                target
            };
            println!("{}: {}", target, msg);
        }
    }

    tokio::try_join!(input_processor, server_messages_processor)?;
    Ok(())
}
