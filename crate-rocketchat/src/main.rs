use std::env;
use std::io::{self, Write};

use rocketchat::{IncomingMessage, MessageSender, RocketChatClient};

fn main() {
    let args: Vec<String> = env::args().collect();
    let config_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "config.toml".to_string()
    };

    eprintln!("Loading config from: {}", config_path);
    let client = match RocketChatClient::from_config_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    // Register any rooms to monitor (optional: room names without @mention)
    // client.register_room("general");

    let bot_name = client.bot_name().to_string();

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let result = rt.block_on(async move {
        eprintln!("Connecting to RocketChat as {}...", bot_name);
        eprintln!("Listening for messages. Press Ctrl+C to quit.");
        eprintln!();

        client
            .connect_and_run(move |msg: IncomingMessage, sender: MessageSender| {
                async move {
                    // Print incoming message
                    let location = if msg.is_dm {
                        format!("DM from {}", msg.sender_name)
                    } else {
                        let name = if msg.room_fname.is_empty() {
                            &msg.room_name
                        } else {
                            &msg.room_fname
                        };
                        format!("#{} from {}", name, msg.sender_name)
                    };
                    println!("[{}] {}", location, msg.text);
                    let _ = io::stdout().flush();

                    // Handle built-in commands
                    if msg.text.trim() == "!ping" {
                        let reply = format!("pong @{}", msg.sender_name);
                        if let Err(e) = sender.reply(&reply).await {
                            eprintln!("Failed to send reply: {}", e);
                        }
                    } else if msg.text.trim().starts_with("!echo ") {
                        let echoed = msg.text.trim().strip_prefix("!echo ").unwrap_or("");
                        if let Err(e) = sender.reply(echoed).await {
                            eprintln!("Failed to send reply: {}", e);
                        }
                    } else if msg.text.trim() == "!help" {
                        let help = "Commands: !ping, !echo <text>, !help";
                        if let Err(e) = sender.reply(help).await {
                            eprintln!("Failed to send reply: {}", e);
                        }
                    }
                }
            })
            .await
    });

    match result {
        Ok(()) => eprintln!("Connection closed."),
        Err(e) => eprintln!("Error: {}", e),
    }
}
