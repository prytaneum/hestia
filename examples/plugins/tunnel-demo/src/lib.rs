use hestia::{Error, Plugin, Value};

/// Starts the agent when a server starts and stops it when that server exits.
struct TunnelDemo;

const SERVER: &str = "server-";

impl Plugin for TunnelDemo {
    fn on_event(topic: &str, payload: Value) {
        let Some(server) = payload["id"].as_str().filter(|id| id.starts_with(SERVER)) else {
            return;
        };
        let outcome = match topic {
            "process.started" => up(server),
            "process.exit" => down(server),
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            hestia::log::warn(&format!("tunnel for {server}: {error}"));
        }
    }
}

fn up(server: &str) -> Result<(), Error> {
    let agent = hestia::process::spawn("agent", &["86400"], &[])?;
    hestia::log::info(&format!("tunnel up for {server}"));
    hestia::data::save(server, &agent)
}

fn down(server: &str) -> Result<(), Error> {
    let Some(agent) = hestia::data::load::<String>(server)? else {
        return Ok(());
    };
    hestia::process::stop(&agent)?;
    hestia::log::info(&format!("tunnel down for {server}"));
    Ok(())
}

hestia::plugin!(TunnelDemo);
