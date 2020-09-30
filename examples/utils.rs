pub struct Args {
    pub is_server: bool,
}

pub fn parse_args() -> Args {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let is_server = false;
        } else {
            let args: Vec<String> = std::env::args().collect();

            if args.len() < 2 {
                panic!("Need to select to run as either a server (--server) or a client (--client).");
            }

            let connection_type = &args[1];

            let is_server = match connection_type.as_str() {
                "--server" | "-s" => true,
                "--client" | "-c" => false,
                _ => panic!("Need to select to run as either a server (--server) or a client (--client)."),
            };
        }
    }

    Args { is_server }
}
