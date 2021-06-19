pub struct Args {
    pub is_server: bool,
    pub manual_flush: bool,
}

pub fn parse_args() -> Args {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let is_server = false;
            let manual_flush = false;
        } else {
            let args: Vec<String> = std::env::args().collect();

            if args.len() < 2 {
                panic!("Need to select to run as either a server (--server) or a client (--client), optionally with a second arg of either --auto-flush or --manual-flush");
            }

            let connection_type = &args[1];

            let is_server = match connection_type.as_str() {
                "--server" | "-s" => true,
                "--client" | "-c" => false,
                _ => panic!("Need to select to run as either a server (--server) or a client (--client)."),
            };

            let manual_flush = if args.len() == 3 {
                let flushing = &args[2];
                match flushing.as_str() {
                    "--auto-flush" => false,
                    "--manual-flush" => true,
                    _ => false,
                }
            } else {
                false
            };
        }
    }

    Args { is_server, manual_flush }
}
