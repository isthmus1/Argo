//! Where the database lives: the address the `spacetime` CLI publishes to and the WebSocket URI
//! SDK clients (this server, workers, the GUI) connect to.

use std::net::IpAddr;

/// SpacetimeDB's hosted cloud, as the CLI's built-in `maincloud` server nickname names it.
const MAINCLOUD: &str = "https://maincloud.spacetimedb.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// `--server` for `spacetime publish` and `spacetime call`: an `http(s)://` URL.
    pub cli: String,
    /// What SDK clients connect to: the same host as `ws(s)://`.
    pub ws: String,
}

impl Endpoint {
    /// An existing host: `http://host:port`, `https://host`, or `maincloud`.
    pub fn remote(server: &str) -> Result<Endpoint, String> {
        let cli = match server.trim_end_matches('/') {
            "maincloud" => MAINCLOUD.to_string(),
            s if s.starts_with("http://") || s.starts_with("https://") => s.to_string(),
            s => {
                return Err(format!(
                    "--server {s}: expected http://, https:// or maincloud"
                ));
            }
        };
        let ws = cli.replacen("http", "ws", 1);
        Ok(Endpoint { cli, ws })
    }

    /// The host this server starts itself, reached locally whatever address it listens on.
    pub fn local(port: u16) -> Endpoint {
        Endpoint {
            cli: format!("http://127.0.0.1:{port}"),
            ws: format!("ws://127.0.0.1:{port}"),
        }
    }
}

/// The URI to give workers and the GUI for a host listening on `listen:port`.
pub fn advertised(listen: IpAddr, port: u16) -> String {
    if listen.is_unspecified() {
        format!("ws://<this machine's address>:{port}")
    } else {
        format!("ws://{}", std::net::SocketAddr::new(listen, port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_servers_map_to_websocket_uris() {
        let e = Endpoint::remote("http://10.0.0.5:3000/").unwrap();
        assert_eq!(
            (e.cli.as_str(), e.ws.as_str()),
            ("http://10.0.0.5:3000", "ws://10.0.0.5:3000")
        );
        let e = Endpoint::remote("https://db.example.com").unwrap();
        assert_eq!(e.ws, "wss://db.example.com");
        let e = Endpoint::remote("maincloud").unwrap();
        assert_eq!(e.ws, "wss://maincloud.spacetimedb.com");
        assert!(Endpoint::remote("db.example.com").is_err());
    }

    #[test]
    fn advertised_uris() {
        assert_eq!(
            advertised("0.0.0.0".parse().unwrap(), 3100),
            "ws://<this machine's address>:3100"
        );
        assert_eq!(
            advertised("127.0.0.1".parse().unwrap(), 3100),
            "ws://127.0.0.1:3100"
        );
        assert_eq!(advertised("::1".parse().unwrap(), 3100), "ws://[::1]:3100");
    }
}
