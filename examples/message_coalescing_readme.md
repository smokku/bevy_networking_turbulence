# Notes on message coalescing example

* If you send a `packet` it is dispatched immediately.
* If you send a `message`, which must be smaller than the maximum packet size, turbulence will coalesce multiple messages into a single packet.
* When you call turbulence's `flush()` function on a channel, all pending outbound messages are coalesced into as few packets as possible and sent.
* Default behaviour of `bevy_networking_turbulence` is to call flush after every `send_message` or `broadcast_message`
* You can change this to flush once per tick instead, which may reduce the number of packets you are sending.

## Auto-flushing channels (default)

`env RUST_LOG=info cargo run --example message_coalescing -- --server --auto-flush`

```
[Server responds with a PONG for each PING it receives]
2021-06-19 13:47:42,390 INFO  [message_coalescing] Final stats, is_server: true, flushing mode: --auto-flush
2021-06-19 13:47:42,390 INFO  [message_coalescing] PingPongCounter { pings_sent: 0, pings_seen: 100, pongs_sent: 100, pongs_seen: 0 }
2021-06-19 13:47:42,390 INFO  [message_coalescing] PacketStats { packets_tx: 10, packets_rx: 89, bytes_tx: 410, bytes_rx: 489 } [h:0]
2021-06-19 13:47:42,390 INFO  [message_coalescing] Exiting.
```

`env RUST_LOG=info cargo run --example message_coalescing -- --client --auto-flush`

```
[Client sends 10 PINGS per tick, for 10 ticks.]
2021-06-19 13:47:40,535 INFO  [message_coalescing] Final stats, is_server: false, flushing mode: --auto-flush
2021-06-19 13:47:40,535 INFO  [message_coalescing] PingPongCounter { pings_sent: 100, pings_seen: 0, pongs_sent: 0, pongs_seen: 100 }
2021-06-19 13:47:40,535 INFO  [message_coalescing] PacketStats { packets_tx: 89, packets_rx: 10, bytes_tx: 489, bytes_rx: 410 } [h:0]
2021-06-19 13:47:40,535 INFO  [message_coalescing] Exiting.
```

Due to how the timing works out, some client pings are coalesced (only 89 packets sent, not 100).
The server has 10 pings waiting to be read at each tick, and sends 10 pong msgs within the handle_messages system before turbulence has a chance to flush, so has a chance to coalesce messages into fewer packets.


## Manually flushing channels after each tick

```rust
NetworkingPlugin{message_flushing_strategy = MessageFlushingStrategy::Never, ..Default::default()}

// and add a system to flush:

app.add_system_to_stage(CoreStage::PostUpdate, flush_channels.system());

fn flush_channels(mut net: ResMut<NetworkResource>) {
    for (_handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        channels.flush::<NetMsg>();
        channels.flush::<OtherMsgTypes>(); // etc
    }
}

```

`env RUST_LOG=info cargo run --example message_coalescing -- --server --manual-flush`

```
[Server responds with a PONG for each PING it receives]
2021-06-19 14:14:27,219 INFO  [message_coalescing] Final stats, is_server: true, flushing mode: --auto-flush
2021-06-19 14:14:27,219 INFO  [message_coalescing] PingPongCounter { pings_sent: 0, pings_seen: 100, pongs_sent: 100, pongs_seen: 0 }
2021-06-19 14:14:27,219 INFO  [message_coalescing] PacketStats { packets_tx: 10, packets_rx: 10, bytes_tx: 410, bytes_rx: 410 } [h:0]
2021-06-19 14:14:27,219 INFO  [message_coalescing] Exiting.
```

`env RUST_LOG=info cargo run --example message_coalescing -- --client --manual-flush`

```
[Client sends 10 PINGS per tick, for 10 ticks.]
2021-06-19 14:14:25,399 INFO  [message_coalescing] Final stats, is_server: false, flushing mode: --manual-flush
2021-06-19 14:14:25,399 INFO  [message_coalescing] PingPongCounter { pings_sent: 100, pings_seen: 0, pongs_sent: 0, pongs_seen: 100 }
2021-06-19 14:14:25,399 INFO  [message_coalescing] PacketStats { packets_tx: 10, packets_rx: 10, bytes_tx: 410, bytes_rx: 410 } [h:0]
2021-06-19 14:14:25,399 INFO  [message_coalescing] Exiting.
```

## Observations

Note how when manually flushing per tick the client only sends 10 packets - one per tick.

In a game server where you might have a few different systems enqueueing messages to be sent to clients,
you may benefit from manually flushing per tick.