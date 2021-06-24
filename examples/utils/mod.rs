// Arg parser for the examples.
// 
// each example gets a different Args struct, which it adds as a bevy resource

use clap::{Arg, App as ClapApp, value_t_or_exit};

#[derive(Debug)]
pub struct SimpleArgs {
    pub is_server: bool,
}

#[derive(Debug)]
pub struct MessageCoalescingArgs {
    pub is_server: bool,
    pub pings: usize,
    pub pongs: usize,
    pub manual_flush: bool,
    pub auto_flush: bool,
}

#[derive(Debug)]
pub struct IdleTimeoutArgs {
    pub is_server: bool,
    pub pings: usize,
    pub pongs: usize,
    pub idle_timeout_ms: Option<usize>,
    pub auto_heartbeat_ms: Option<usize>,
}

#[allow(dead_code)]
pub fn parse_simple_args() -> SimpleArgs {
    let matches = ClapApp::new(std::env::current_exe().file_name().to_string_lossy())
        .about("Simple example just sends some packets")
        .args(server_or_client_args().as_slice())
        .get_matches();
    SimpleArgs {
        is_server: matches.is_present("server")
    }
}

#[allow(dead_code)]
pub fn parse_message_coalescing_args() -> MessageCoalescingArgs {
    let matches = ClapApp::new("")
        .about("Message coalescing example")
        .args(server_or_client_args().as_slice())
        .args(flushing_strategy_args().as_slice())
        .args(pings_pongs_args().as_slice())
        .get_matches();
    MessageCoalescingArgs {
        is_server: matches.is_present("server"),
        pings: value_t_or_exit!(matches, "pings", usize),
        pongs: value_t_or_exit!(matches, "pongs", usize),
        manual_flush: matches.is_present("manual-flush"),
        auto_flush: matches.is_present("auto-flush"),
    }
}

#[allow(dead_code)]
pub fn parse_idle_timeout_args() -> IdleTimeoutArgs {
    let matches = ClapApp::new("")
        .about("Idle timeout example")
        .args(server_or_client_args().as_slice())
        .args(pings_pongs_args().as_slice())
        .args(timeout_args().as_slice())
        .get_matches();
    let idle_timeout_ms = if matches.occurrences_of("idle-drop-timeout") == 1 {
        Some(value_t_or_exit!(matches, "idle-drop-timeout", usize))
    } else {
        None
    };
    let auto_heartbeat_ms = if matches.occurrences_of("heartbeat-interval") == 1 {
        Some(value_t_or_exit!(matches, "heartbeat-interval", usize))
    } else {
        None
    };
    IdleTimeoutArgs {
        is_server: matches.is_present("server"),
        pings: value_t_or_exit!(matches, "pings", usize),
        pongs: value_t_or_exit!(matches, "pongs", usize),
        idle_timeout_ms,
        auto_heartbeat_ms,
    }
}

fn server_or_client_args<'a>() -> Vec<Arg<'a, 'a>> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            // will default to client, unless server specified.
            // wasm32 builds can't be a server.
            vec![]
        } else {
            vec![
                Arg::with_name("server")
                .help("Listen as a server")
                .long("server")
                .required_unless("client")
                .conflicts_with("client")
                .takes_value(false)
                ,
                Arg::with_name("client")
                .help("Connect as a client")
                .long("client")
                .required_unless("server")
                .conflicts_with("server")
                .takes_value(false)
            ]
        }
    }
}

#[allow(dead_code)]
fn timeout_args<'a>() -> Vec<Arg<'a, 'a>> {
    vec![
        Arg::with_name("idle-drop-timeout")
        .help("Idle timeout (ms) after which to drop inactive connections")
        .long("idle-drop-timeout")
        .default_value("3000")
        .takes_value(true)
        .required(false)
        ,
        Arg::with_name("heartbeat-interval")
        .help("Interval (ms) after which to send a heartbeat packet, if we've been silent this long")
        .long("heartbeat-interval")
        .default_value("1000")
        .takes_value(true)
        .required(false)
    ]   
}

#[allow(dead_code)]
fn flushing_strategy_args<'a>() -> Vec<Arg<'a, 'a>> {
    vec![
        Arg::with_name("auto-flush")
        .help("Flush after every send")
        .long("auto-flush")
        .required_unless("manual-flush")
        .conflicts_with("manual-flush")
        .takes_value(false)
        ,
        Arg::with_name("manual-flush")
        .help("No automatic flushing, you must add a flushing system")
        .long("manual-flush")
        .required_unless("auto-flush")
        .conflicts_with("auto-flush")
        .takes_value(false)
    ]
}

#[allow(dead_code)]
fn pings_pongs_args<'a>() -> Vec<Arg<'a, 'a>> {
    vec![
        Arg::with_name("pings")
        .long("pings")
        .default_value("0")
        .help("Number of pings to send, once connected")
        .takes_value(true)
        ,
        Arg::with_name("pongs")
        .long("pongs")
        .default_value("0")
        .help("Number of pongs available to send as replies to pings")
        .takes_value(true)
    ]
}
