use futures::prelude::*;
use irc::client::prelude::*;
use serde::{Deserialize, Serialize};
use std::{fs, io::stdin, path::Path};
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
let mut port=String::new();
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
        channels: config.channels,
        ..Config::default()
    };

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;
    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        print!("{}", message);
        match message.command {
            Command::PRIVMSG(ref target, ref msg) => {
                println!("{}: {}", target, msg);
            }
            _ => {}
        }
    }
    Ok(())
}
