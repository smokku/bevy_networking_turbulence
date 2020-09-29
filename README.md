# bevy_networking_turbulence

Networking plugin for [Bevy engine][1] running on [naia-socket][2] and [turbulence][3] libraries.

Early work-in-progress.

[1]: https://github.com/bevyengine/bevy
[2]: https://github.com/naia-rs/naia-socket
[3]: https://github.com/kyren/turbulence

## Testing

### Native

On one terminal run:

    $ env RUST_LOG=debug cargo run --example simple --features use-udp -- --server

On other terminal run:

    $ env RUST_LOG=debug cargo run --example simple --features use-udp -- --client

### WASM

On one terminal run:

    $ env RUST_LOG=debug cargo run --example simple --features use-webrtc -- --server

Change IP address in `examples/simple.rs` / `startup()` function to point to your local machine, and run:

    $ cargo build --example simple --target wasm32-unknown-unknown
    $ wasm-bindgen --out-dir target --target web target/wasm32-unknown-unknown/debug/examples/simple.wasm
    $ basic-http-server .

Open <http://127.0.0.1:4000>.
