# Notes on idle_timeout example

* `Idle timeout` means if no packets received for the specified duration, we drop the connection.
* `Heartbeat` is a packet sent automatically, and discarded by the remote peer, if we have otherwise not sent anything for the specified duration. Heartbeat packets exist to prevent the other peer from idle-timeout dropping you.
* By default, there are no idle timeouts or heartbeats.
* Heartbeat packets are standard UDP packets (not turbulence messages) and as such may not arrive.


## Idle timeout, to drop inactive connections

Run the server with a 3 second idle-drop-timeout:

`env RUST_LOG=info cargo run --example idle_timeout -- --server --pongs 2 --idle-drop-timeout 3000`

```
2021-06-24 11:56:40,864 INFO  [idle_timeout] IdleTimeoutArgs { is_server: true, pings: 0, pongs: 2, idle_timeout_ms: Some(3000), auto_heartbeat_ms: None }
2021-06-24 11:56:40,867 INFO  [idle_timeout] Starting server
... run the client ...
2021-06-24 11:56:46,063 INFO  [idle_timeout] Other event: Connected(0)
2021-06-24 11:56:46,063 INFO  [idle_timeout] Got packet on [0]: PING
2021-06-24 11:56:46,064 INFO  [idle_timeout] Sent PONG
2021-06-24 11:56:47,072 INFO  [idle_timeout] Got packet on [0]: PING
2021-06-24 11:56:47,072 INFO  [idle_timeout] Sent PONG
2021-06-24 11:56:50,388 WARN  [bevy_networking_turbulence] Idle disconnect for h:0
2021-06-24 11:56:50,405 INFO  [idle_timeout] Other event: Error(0, MissedHeartbeat)
2021-06-24 11:56:50,406 INFO  [idle_timeout] Other event: Disconnected(0)
```

Run a client:

`env RUST_LOG=info cargo run --example idle_timeout -- --client --pings 3`

```
2021-06-24 11:56:45,040 INFO  [idle_timeout] IdleTimeoutArgs { is_server: false, pings: 2, pongs: 0, idle_timeout_ms: None, auto_heartbeat_ms: None }
2021-06-24 11:56:45,043 INFO  [idle_timeout] Starting client
2021-06-24 11:56:45,063 INFO  [idle_timeout] Other event: Connected(0)
2021-06-24 11:56:46,089 INFO  [idle_timeout] Got packet on [0]: PONG
2021-06-24 11:56:47,054 INFO  [idle_timeout] (No more pings left to send)
2021-06-24 11:56:47,095 INFO  [idle_timeout] Got packet on [0]: PONG
```

Note that 3 seconds after the last PING was received by the server, it drops the connection. The client has no idea this has happened, because dropping a connection doesn't attempt to signal the remote peer.

If you restart the client now, after the server has dropped it, the client will be assigned a new `ConnectionHandle` and proceed as normal. 

## Making the client detect dropped connections

Set the same `--idle-drop-timeout` value on the client, and it will also get a `NetworkError::MissedHeartbeat` followed immediately by a `NetworkError:Disconnected`.

## Using heartbeats to keep connections open

If your higher level protocol doesn't guarantee messages will be sent frequently enough, you can enable auto heartbeats. Since they can be lost (plain udp packets), setting the heartbeat timeout to less than a half of the idle-drop-timeout gives you a second chance if the first heartbeat packet is lost.

Nothing is logged at the `INFO` level for heartbeats, but if you run a client like this at `DEBUG`:

`env RUST_LOG=info cargo run --example idle_timeout -- --client --pings 3 --heartbeat-interval 1000`

```
2021-06-24 12:11:45,315 INFO  [idle_timeout] IdleTimeoutArgs { is_server: false, pings: 3, pongs: 0, idle_timeout_ms: None, auto_heartbeat_ms: Some(1000) }
2021-06-24 12:11:45,318 INFO  [idle_timeout] Starting client
2021-06-24 12:11:45,319 INFO  [idle_timeout] Other event: Connected(0)
2021-06-24 12:11:45,830 DEBUG [bevy_networking_turbulence] millis since last rx: 511 tx: 511
2021-06-24 12:11:46,321 DEBUG [bevy_networking_turbulence] millis since last rx: 1002 tx: 0
2021-06-24 12:11:46,357 DEBUG [bevy_networking_turbulence] Received on [0] 4 RAW: PONG
2021-06-24 12:11:46,357 DEBUG [bevy_networking_turbulence] Processing as packet
2021-06-24 12:11:46,358 INFO  [idle_timeout] Got packet on [0]: PONG
2021-06-24 12:11:46,823 DEBUG [bevy_networking_turbulence] millis since last rx: 465 tx: 502
2021-06-24 12:11:47,334 DEBUG [bevy_networking_turbulence] millis since last rx: 976 tx: 0
2021-06-24 12:11:47,352 DEBUG [bevy_networking_turbulence] Received on [0] 4 RAW: PONG
2021-06-24 12:11:47,352 DEBUG [bevy_networking_turbulence] Processing as packet
2021-06-24 12:11:47,352 INFO  [idle_timeout] Got packet on [0]: PONG
2021-06-24 12:11:47,832 DEBUG [bevy_networking_turbulence] millis since last rx: 480 tx: 498
2021-06-24 12:11:48,335 INFO  [idle_timeout] (No more pings left to send)
2021-06-24 12:11:48,335 DEBUG [bevy_networking_turbulence] millis since last rx: 983 tx: 0
2021-06-24 12:11:48,354 DEBUG [bevy_networking_turbulence] Received on [0] 4 RAW: PONG
2021-06-24 12:11:48,354 DEBUG [bevy_networking_turbulence] Processing as packet
2021-06-24 12:11:48,355 INFO  [idle_timeout] Got packet on [0]: PONG
2021-06-24 12:11:48,829 DEBUG [bevy_networking_turbulence] millis since last rx: 474 tx: 493
2021-06-24 12:11:49,333 DEBUG [bevy_networking_turbulence] millis since last rx: 978 tx: 998
2021-06-24 12:11:49,828 DEBUG [bevy_networking_turbulence] millis since last rx: 1473 tx: 1493
2021-06-24 12:11:49,828 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:50,331 DEBUG [bevy_networking_turbulence] millis since last rx: 1976 tx: 502
2021-06-24 12:11:50,833 DEBUG [bevy_networking_turbulence] millis since last rx: 2478 tx: 1005
2021-06-24 12:11:50,833 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:51,329 DEBUG [bevy_networking_turbulence] millis since last rx: 2974 tx: 495
2021-06-24 12:11:51,824 DEBUG [bevy_networking_turbulence] millis since last rx: 3469 tx: 990
2021-06-24 12:11:52,337 DEBUG [bevy_networking_turbulence] millis since last rx: 3982 tx: 1503
2021-06-24 12:11:52,337 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:52,832 DEBUG [bevy_networking_turbulence] millis since last rx: 4477 tx: 495
2021-06-24 12:11:53,323 DEBUG [bevy_networking_turbulence] millis since last rx: 4968 tx: 986
2021-06-24 12:11:53,831 DEBUG [bevy_networking_turbulence] millis since last rx: 5476 tx: 1494
2021-06-24 12:11:53,831 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:54,323 DEBUG [bevy_networking_turbulence] millis since last rx: 5968 tx: 492
2021-06-24 12:11:54,833 DEBUG [bevy_networking_turbulence] millis since last rx: 6478 tx: 1001
2021-06-24 12:11:54,833 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:55,336 DEBUG [bevy_networking_turbulence] millis since last rx: 6981 tx: 503
2021-06-24 12:11:55,828 DEBUG [bevy_networking_turbulence] millis since last rx: 7473 tx: 994
2021-06-24 12:11:56,331 DEBUG [bevy_networking_turbulence] millis since last rx: 7976 tx: 1497
2021-06-24 12:11:56,331 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:56,832 DEBUG [bevy_networking_turbulence] millis since last rx: 8477 tx: 500
2021-06-24 12:11:57,338 DEBUG [bevy_networking_turbulence] millis since last rx: 8983 tx: 1006
2021-06-24 12:11:57,338 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:57,826 DEBUG [bevy_networking_turbulence] millis since last rx: 9471 tx: 487
2021-06-24 12:11:58,324 DEBUG [bevy_networking_turbulence] millis since last rx: 9969 tx: 985
2021-06-24 12:11:58,835 DEBUG [bevy_networking_turbulence] millis since last rx: 10479 tx: 1495
2021-06-24 12:11:58,835 DEBUG [bevy_networking_turbulence] Sending hearbeat packet on h:0
2021-06-24 12:11:59,322 DEBUG [bevy_networking_turbulence] millis since last rx: 10966 tx: 486
2021-06-24 12:11:59,822 DEBUG [bevy_networking_turbulence] millis since last rx: 11467 tx: 987
```

Note the following things:

* The server doesn't drop the connection, due to heartbeat packets the client sends.
* After the 3 ping/pong exchange, the time since last rx keeps increasing. This is because the server does not have heartbeats enabled, so isn't sending anything to the client.
* The last tx time is as much as 1500ms in some cases, even though we specified 1000ms for heartbeats. This is because the bevy system that checks for idle connections and sends heartbeats runs on a fixed timestep, defaulting to 0.5 seconds. This can be configured in the `NetworkingPlugin` like so:

```rust
pub struct NetworkingPlugin {
    /// FixedTimestep for the `heartbeats_and_timeouts` system which checks for idle connections
    /// and sends heartbeats. Does not need to be every frame.
    ///
    /// The `heartbeats_and_timeouts` system is only added if `idle_timeout_ms` or `auto_heartbeat_ms` are specified.
    ///
    /// Default if None: 0.5 secs
    pub heartbeats_and_timeouts_timestep_in_seconds: Option<f64>,
}
```
