# TCP Chat — Rust Learning Project

This project is a small terminal chat system. A server accepts TCP connections, and each client sends terminal text to the server. The server relays every received message to every connected client, including the sender.

It is intentionally small: it demonstrates Rust ownership, threads, channels, TCP sockets, byte buffers, and basic error handling without external crates.

## Project layout

```text
chat/
├── server/                 # Broadcast server binary
│   └── src/main.rs
└── client/                 # Terminal client binary
    └── src/main.rs
```

`server` and `client` are separate Cargo packages, not members of one Cargo workspace. Each directory therefore has its own `Cargo.toml` and is run from that directory (or with its manifest path).

## Run it

Install the Rust toolchain with Cargo, then use separate terminals:

```sh
# Terminal 1
cd chat/server
cargo run

# Terminal 2 (repeat in further terminals for more clients)
cd chat/client
cargo run
```

The server binds to `127.0.0.1:6000`. This loopback address allows connections only from the same computer. Type a message and press Enter; type `:quit` in a client to exit it.

## Message journey

```text
stdin → client channel → client TCP stream → server reader thread
      → server channel → server main loop → every client TCP stream
      → client reader thread → terminal output
```

1. The client main thread blocks on `stdin` with `read_line`.
2. It trims the newline and sends the resulting `String` through an mpsc channel.
3. Its networking thread receives the string, encodes it as bytes, pads it to 32 bytes, and writes those bytes to the TCP stream.
4. The server gives every accepted connection a reader thread. That thread reads one 32-byte frame, converts it to a UTF-8 `String`, and sends it through another channel.
5. The server main loop receives the string and calls `write_all` for each saved client socket.
6. Each client networking thread reads a frame and prints what it received.

`mpsc` means *multiple producer, single consumer*. Several server reader threads can clone and use `tx`; the server main loop alone owns `rx`. This avoids sharing the `clients` list between reader threads.

## The wire format

TCP is a stream of bytes, not a message protocol. A call to `write_all` can be read in pieces, or combined with later writes. This project defines a simple boundary rule:

```text
Message: "hi"
Bytes:   [104, 105, 0, 0, 0, ...]  # exactly 32 bytes
```

`MSG_SIZE` is `32` in both programs. Sending code uses `String::into_bytes()` and `Vec::resize(MSG_SIZE, 0)` to produce a zero-padded buffer. Receiving code uses `read_exact(&mut buff)` so it attempts to obtain exactly 32 bytes, then `take_while(|&x| x != 0)` removes the padding.

The client accepts only input that fits in 32 **bytes**, not 32 characters. `resize` does not truncate an overlong vector, so a longer message is written as more than one frame and will corrupt framing for later reads. Keep input below 32 bytes when using the project as written.

## Server walkthrough

The server imports traits and types from the standard library:

```rust
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
```

- `TcpListener` listens for new network connections.
- `Read` and `Write` are traits that provide `read_exact` and `write_all` on sockets.
- `thread::spawn` runs independent connection readers.
- `mpsc` transfers owned messages safely between those threads.

### Set up the listener

```rust
let server = TcpListener::bind(LOCAL).expect("Listener failed to bind");
server.set_nonblocking(true).expect("Failed to set non-blocking");
```

`bind` reserves the address and port. It returns `Result<TcpListener, io::Error>` because the port may already be in use. `expect` unwraps the successful value but terminates with the supplied message on failure. This is convenient for a learning program; a production server would normally return or log the error instead.

Non-blocking mode means `accept()` returns immediately when no client is waiting, rather than pausing the whole loop. The loop then sleeps for 100 ms to avoid continuously consuming a CPU core.

### Accept a connection and preserve a writer

```rust
if let Ok((mut socket, addr)) = server.accept() {
    let tx = tx.clone();
    clients.push(socket.try_clone().expect("Failed to clone socket"));
    thread::spawn(move || { /* read from socket */ });
}
```

`if let Ok(...)` handles only a successful accept; an expected non-blocking “no connection yet” error is ignored. `addr` is the peer address used for logging.

A `TcpStream` has ownership like any other Rust value. The reader thread needs to own one socket handle, while the main loop needs a handle for broadcasting. `try_clone()` creates another handle to the same underlying connection. The `move` closure then transfers ownership of `socket`, `addr`, and the cloned sender into the new thread. Rust enforces this at compile time, preventing an accidental use of a value after it was moved.

### Read and forward

Each connection thread repeatedly creates a 32-byte `Vec<u8>` and calls:

```rust
socket.read_exact(&mut buff)
```

On success, it removes zero padding, converts the remaining `Vec<u8>` with `String::from_utf8`, logs it, and calls `tx.send(msg)`. Sending a `String` transfers ownership to the channel; no shared mutable message state is needed.

The `WouldBlock` match arm expresses an intended non-blocking error path. However, the code only sets the **listener** non-blocking; it does not call `socket.set_nonblocking(true)`. Accepted streams therefore normally block in `read_exact` until a whole frame arrives. That is acceptable for its dedicated reader thread, but the `WouldBlock` arm is not normally used.

### Broadcast and remove dead writers

```rust
clients = clients.into_iter().filter_map(|mut client| {
    client.write_all(&buff).map(|_| client).ok()
}).collect::<Vec<_>>();
```

`into_iter()` consumes the old vector, so each socket can be moved into the closure. `write_all` returns `Result<(), io::Error>`:

- successful writes map back to `client`, retaining that connection;
- failed writes become `None` through `.ok()`;
- `filter_map` drops `None` entries and collects the working sockets into a new vector.

This is a compact Rust pattern for cleanup during broadcasting. A failed client is silently removed.

## Client walkthrough

The client connects and sets its stream non-blocking:

```rust
let mut client = TcpStream::connect(LOCAL).expect("Failed to connect to server");
client.set_nonblocking(true).expect("Failed to initiate non-blocking");
```

`TcpStream::connect` establishes a TCP connection. Again, `expect` makes connection failure fatal, so the server must already be running.

### Separate terminal input from socket work

The client creates `let (tx, rx) = mpsc::channel::<String>();`. The main thread owns `tx` and waits for user input. The spawned thread owns `rx` and the socket. This separation keeps the terminal responsive while network reads happen independently.

```rust
let msg = buff.trim().to_string();
if msg == ":quit" || tx.send(msg).is_err() { break; }
```

- `trim()` removes the newline added by `read_line`.
- `to_string()` creates an owned `String` suitable for sending.
- `:quit` is handled locally and is not sent to the server.
- `is_err()` also exits if the networking thread has ended and the receiver has been dropped.

Inside the networking thread, `rx.try_recv()` checks for outgoing input without blocking. Its `Empty` result is normal; the loop then sleeps for 100 ms. When it receives a string, it pads and writes the frame.

### Receive messages

The same thread calls `client.read_exact(&mut buff)` before checking `rx`, then prints received data. Unlike the server, the client currently prints the decoded byte vector:

```rust
println!("Received: {:?}", msg);
```

So `hello` is displayed as `[104, 101, 108, 108, 111]`, not as `hello`. The server already demonstrates the needed conversion: `String::from_utf8(msg)`.

## Rust concepts used

| Concept | Where it appears | Why it matters |
| --- | --- | --- |
| `const` | `LOCAL`, `MSG_SIZE` | Gives both programs named, immutable configuration values. |
| `Result` | `bind`, `connect`, `read_exact`, `write_all` | Makes I/O failure explicit instead of using exceptions. |
| Traits | `Read`, `Write` imports | Make socket read/write methods available. |
| Ownership and `move` | server reader thread | Ensures a thread owns the values it uses. |
| `Vec<u8>` | message buffers | Represents mutable network bytes. |
| `String` / UTF-8 | server conversion | Represents valid Rust text after decoding bytes. |
| Threads | `thread::spawn` | Allows each connection or concern to make progress independently. |
| Channels | `mpsc::channel` | Move messages safely across thread boundaries. |
| Closures and iterators | broadcast `filter_map` | Transform the client list while removing failed connections. |

## Development notes and next steps

This is a useful starting point, but it is not yet a robust chat protocol.

1. **Enforce the size limit.** Reject or truncate messages longer than `MSG_SIZE` before writing. A length-prefixed format is a better long-term protocol.
2. **Fix non-blocking reads.** `read_exact` on a non-blocking client can receive part of a frame and then return `WouldBlock`; this code discards the partial bytes. Either use blocking reads in the dedicated client thread, or retain partial bytes until a full frame is assembled.
3. **Decode client output.** Convert received bytes using `String::from_utf8` (or loss-tolerant `from_utf8_lossy`) before printing.
4. **Avoid polling.** The 100 ms sleep loops trade latency for simplicity. A production version could use blocking channel receives, `select`-style coordination, or an async runtime such as Tokio.
5. **Improve errors and shutdown.** Replace `expect` where recovery is useful, log write failures, and give reader threads a coordinated shutdown path.
6. **Add chat features.** Usernames, join/leave notices, targeted messages, timestamps, and tests for framing are natural next steps.

## Useful commands

```sh
# Compile without running
cargo check --manifest-path chat/server/Cargo.toml
cargo check --manifest-path chat/client/Cargo.toml

# Format the source
cargo fmt --manifest-path chat/server/Cargo.toml
cargo fmt --manifest-path chat/client/Cargo.toml
```
