# bevy_networking_turbulence

Networking plugin for [Bevy engine][1] running on [naia-socket][2] and [turbulence][3] libraries.

Combination of _naia-socket_ and _turbulence_ allows for exchange of raw messages over UDP or UDP-like connection (over WebRTC),
or building unreliable/reliable channels of structured messages over such UDP/UDP-like messages.

This plugin works both in native (Linux, Windows, MacOS) over UDP packets
and in Browser/WASM over UDP-like messages in WebRTC channel.

Still unfinished, but main features are working. For details see [Milestones][4].

[1]: https://github.com/bevyengine/bevy
[2]: https://github.com/amethyst/naia-socket
[3]: https://github.com/kyren/turbulence
[4]: https://github.com/smokku/bevy_networking_turbulence/milestones

## Testing

### Native

On one terminal run:

    $ env RUST_LOG=debug cargo run --example simple -- --server

On other terminal run:

    $ env RUST_LOG=debug cargo run --example simple -- --client

Observe `PING`/`PONG` exchange between server and client. You can run more clients in more terminals.

### WASM

On one terminal run:

    $ env RUST_LOG=debug cargo run --example simple --no-default-features --features use-webrtc -- --server

Change IP address in `examples/simple.rs` / `startup()` function to point to your local machine, and run:

    $ cargo build --example simple --target wasm32-unknown-unknown --no-default-features --features use-webrtc
    $ wasm-bindgen --out-dir target --target web target/wasm32-unknown-unknown/debug/examples/simple.wasm

Serve project directory over HTTP. For example (`cargo install basic-http-server`):

    $ basic-http-server .

Open <http://127.0.0.1:4000> and watch Browser's console in Developer Tools.
You will see the same `PING`/`PONG` exchange as in the Native mode.

### Channels

On one terminal run:

    $ env RUST_LOG=debug cargo run --example channels --features use-udp,bevy/default -- --server

On second terminal run:

    $ env RUST_LOG=warn cargo run --example channels --features use-udp,bevy/default -- --client

On third (and fourth, and more...) terminal run:

    $ env RUST_LOG=warn cargo run --example channels --features use-udp,bevy/default -- --client

You can focus any of client windows and use `Arrow-Left`/`Arrow-Right` cursor keys to change "ball" trajectory.
Observe your controlled ball gets synchronized to all other clients.

#### WASM with Graphics and Channels networking

Due to additional dependencies required, we've decided to keep the browser based graphical channels example in a separate repository.
See https://github.com/ostwilkens/arugio project. It evolved a bit from the simple square balls example and may serve as a basis of your game.
