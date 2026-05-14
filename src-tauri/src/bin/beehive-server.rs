fn main() {
    let config = match beehive_lib::http_server::ServerConfig::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("Beehive server configuration error: {message}");
            std::process::exit(2);
        }
    };

    if let Err(message) = beehive_lib::http_server::run_server(config) {
        eprintln!("Beehive server failed: {message}");
        std::process::exit(1);
    }
}
