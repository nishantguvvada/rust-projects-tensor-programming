# TCP Chat

A small Rust chat example with a TCP server that relays messages to every connected client.

## Parts

- `server/`: Listens on `127.0.0.1:6000`, accepts clients, reads each client's messages in a thread, and broadcasts them to all connected sockets.
- `client/`: Reads terminal input, sends it to the server, and prints messages received from the server. Type `:quit` to exit.

## How it works

Messages use a fixed 32-byte frame: text is padded with zero bytes before sending, then the receiver removes the padding. The server passes incoming messages through an mpsc channel to its main loop, which writes each message to every client.

## Run

Start the server, then start one or more clients in separate terminals:

```sh
cd server && cargo run
cd client && cargo run
```

## Limits

Messages must fit in 32 bytes; longer input can break framing. This is a learning example: it has no usernames, authentication, or robust disconnect/error handling.
