extern crate hyper;
extern crate websocket;

use hyper::net::Fresh;
use hyper::server::request::Request;
use hyper::server::response::Response;
use hyper::Server as HttpServer;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use websocket::sync::Server;
use websocket::{Message, OwnedMessage};

const HTML: &'static str = include_str!("websockets.html");

// The HTTP server handler
fn http_handler(_: Request, response: Response<Fresh>) {
    let mut response = response.start().unwrap();
    // Send a client webpage
    response.write_all(HTML.as_bytes()).unwrap();
    response.end().unwrap();
}

fn main() {
    let clients = Arc::new(Mutex::new(Vec::new()));

    // Start listening for http connections
    thread::spawn(|| {
        let http_server = HttpServer::http("0.0.0.0:8080").unwrap();
        http_server.handle(http_handler).unwrap();
    });

    // Start listening for WebSocket connections
    let ws_server = Server::bind("0.0.0.0:2794").unwrap();

    for connection in ws_server.filter_map(Result::ok) {
        // Clone the Arc for each connection
        let clients = Arc::clone(&clients);

        // Spawn a new thread for each connection.
        thread::spawn(move || {
            if !connection.protocols().contains(&"rust-websocket".to_string()) {
                connection.reject().unwrap();
                return;
            }

            let mut client = match connection.use_protocol("rust-websocket").accept() {
                Ok(client) => client,
                Err(err) => {
                    eprintln!("Error accepting WebSocket connection: {:?}", err);
                    return;
                }
            };

            let ip = client.peer_addr().unwrap();

            println!("Connection from {}", ip);

            let message = Message::text("Hello");
            if let Err(err) = client.send_message(&message) {
                eprintln!("Error sending initial message to client {}: {:?}", ip, err);
                return;
            }

            let (mut receiver, sender) = client.split().unwrap();

            // Wrap the sender in an Arc<Mutex<Sender>>
            let client_sender = Arc::new(Mutex::new(sender));

            // Add the client_sender to the clients vector
            {
                let mut clients = clients.lock().unwrap();
                clients.push(client_sender.clone());
            }

            for message in receiver.incoming_messages() {
                let message = match message {
                    Ok(msg) => msg,
                    Err(err) => {
                        eprintln!("Error receiving message from client {}: {}", ip, err);
                        break;
                    }
                };

                match message {
                    OwnedMessage::Close(_) => {
                        let message = Message::close();
                        if let Err(err) = client_sender.lock().unwrap().send_message(&message) {
                            eprintln!("Error sending close message to client {}: {:?}", ip, err);
                        }
                        println!("Client {} disconnected", ip);
                        break;
                    }
                    OwnedMessage::Ping(data) => {
                        let message = Message::pong(data);
                        if let Err(err) = client_sender.lock().unwrap().send_message(&message) {
                            eprintln!("Error sending pong message to client {}: {:?}", ip, err);
                        }
                    }
                    _ => {
                        // Send the message to all clients
                        let clients = clients.lock().unwrap();
                        for client_sender in &*clients {
                            if let Err(err) = client_sender.lock().unwrap().send_message(&message) {
                                eprintln!("Error sending message to client: {:?}", err);
                            }
                        }
                    }
                }
            }

            // Remove the client_sender from the clients vector
            {
                let mut clients = clients.lock().unwrap();
                if let Some(position) = clients.iter().position(|client| Arc::ptr_eq(client, &client_sender)) {
                    clients.remove(position);
                }
            }
        });
    }
}
