use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::{Arc, Mutex};

use serde_json;

use crate::basemsg::{BaseMessage, Sendable};
use crate::error::{DeserializationError, ServerError};
use crate::event::Event;
use crate::request::Request;
use crate::response::Response;
use crate::revreqt::ReverseRequest;

/// Handles message encoding and decoding of messages.
///
/// The `Server` is responsible for reading the incoming bytestream and
/// constructing deserialized requests from it, as well as constructing and
/// serializing outgoing messages.
pub struct Server<R: Read, W: Write> {
    input_buffer: BufReader<R>,

    /// A sharable `ServerOutput` object for sending messages and events from
    /// other threads.
    pub output: Arc<Mutex<ServerOutput<W>>>,
}

/// Handles emission of messages through the connection.
///
/// `ServerOutput` is responsible for sending messages to the connection.
/// It's only accessible through a mutex that can be shared with other
/// threads. This makes it possible to send e.g. events while the server is
/// blocked polling requests.
pub struct ServerOutput<W: Write> {
    output_buffer: BufWriter<W>,
    sequence_number: i64,
}

impl<R: Read, W: Write> Server<R, W> {
    /// Construct a new Server using the given input and output streams.
    pub fn new(input: BufReader<R>, output: BufWriter<W>) -> Self {
        let server_output = Arc::new(Mutex::new(ServerOutput {
            output_buffer: output,
            sequence_number: 0,
        }));

        Self {
            input_buffer: input,
            output: server_output,
        }
    }

    /// Wait for a request from the development tool
    ///
    /// This will start reading the `input` buffer that is passed to it and will
    /// try to interpret the incoming bytes according to the DAP protocol.
    pub fn poll_request(&mut self) -> Result<Option<Request>, ServerError> {
        let mut header_buffer = String::new();
        let mut content_length: usize = 0;

        // Parse headers until we get an empty line
        loop {
            header_buffer.clear();
            let bytes_read = self
                .input_buffer
                .read_line(&mut header_buffer)
                .map_err(ServerError::IoError)?;

            if bytes_read == 0 {
                return Ok(None); // EOF
            }

            let trimmed = header_buffer.trim_end();

            // Empty line signals end of headers
            if trimmed.is_empty() {
                break;
            }

            // Parse "Header-Name: value" format
            if let Some(colon_pos) = trimmed.find(':') {
                let (header_name, header_value) = trimmed.split_at(colon_pos);
                match header_name {
                    "Content-Length" => {
                        content_length = header_value[1..] // Skip the ':'
                            .trim()
                            .parse()
                            .map_err(|_| ServerError::HeaderParseError {
                                line: header_buffer.clone(),
                            })?;
                    }
                    other => {
                        return Err(ServerError::UnknownHeader {
                            header: other.to_string(),
                        });
                    }
                }
            } else {
                return Err(ServerError::HeaderParseError {
                    line: header_buffer,
                });
            }
        }

        // Read content
        let mut content = vec![0u8; content_length];
        self.input_buffer
            .read_exact(&mut content)
            .map_err(ServerError::IoError)?;

        let content_str = std::str::from_utf8(&content)
            .map_err(|e| ServerError::ParseError(DeserializationError::DecodingError(e)))?;

        let request: Request = serde_json::from_str(content_str)
            .map_err(|e| ServerError::ParseError(DeserializationError::SerdeError(e)))?;

        Ok(Some(request))
    }

    pub fn send(&mut self, body: Sendable) -> Result<(), ServerError> {
        let mut output = self
            .output
            .lock()
            .map_err(|_| ServerError::OutputLockError)?;
        output.send(body)
    }

    pub fn respond(&mut self, response: Response) -> Result<(), ServerError> {
        self.send(Sendable::Response(response))
    }

    pub fn send_event(&mut self, event: Event) -> Result<(), ServerError> {
        self.send(Sendable::Event(event))
    }

    pub fn send_reverse_request(&mut self, request: ReverseRequest) -> Result<(), ServerError> {
        self.send(Sendable::ReverseRequest(request))
    }
}

impl<W: Write> ServerOutput<W> {
    pub fn send(&mut self, body: Sendable) -> Result<(), ServerError> {
        self.sequence_number += 1;

        let message = BaseMessage {
            seq: self.sequence_number,
            message: body,
        };

        let resp_json = serde_json::to_string(&message).map_err(ServerError::SerializationError)?;

        // Write header and content in a single operation
        write!(
            self.output_buffer,
            "Content-Length: {}\r\n\r\n{}",
            resp_json.len(),
            resp_json
        )
        .map_err(ServerError::IoError)?;

        self.output_buffer.flush().map_err(ServerError::IoError)?;
        Ok(())
    }

    pub fn respond(&mut self, response: Response) -> Result<(), ServerError> {
        self.send(Sendable::Response(response))
    }

    pub fn send_event(&mut self, event: Event) -> Result<(), ServerError> {
        self.send(Sendable::Event(event))
    }

    pub fn send_reverse_request(&mut self, request: ReverseRequest) -> Result<(), ServerError> {
        self.send(Sendable::ReverseRequest(request))
    }
}

#[cfg(test)]
mod tests {

    use std::io::Cursor;

    use serde_json::Value;

    use super::*;
    use crate::request::{AttachOrLaunchArguments, Command, RestartArguments};

    fn simulate_poll_request(input: &str) -> Request {
        let mut server_in = Cursor::new(input.as_bytes().to_vec());
        let server_out = Vec::new();
        let mut server = Server::new(BufReader::new(&mut server_in), BufWriter::new(server_out));

        server.poll_request().unwrap().unwrap()
    }

    #[test]
    fn test_server_init_request() {
        let req = simulate_poll_request(
            "Content-Length: 155\r\n\r\n{\"seq\": 152,\"type\": \"request\",\"command\": \"initialize\",\"arguments\": {\"adapterID\": \"0001e357-72c7-4f03-ae8f-c5b54bd8dabf\", \"clientName\": \"Some Cool Editor\"}}",
        );

        assert_eq!(req.seq, 152);
        assert!(matches!(req.command, Command::Initialize { .. }));
    }

    #[test]
    fn test_server_restart_request() {
        let req = simulate_poll_request(
            "Content-Length: 67\r\n\r\n{\"seq\": 152,\"type\": \"request\",\"command\": \"restart\",\"arguments\": {}}",
        );

        assert!(matches!(
            req.command,
            Command::Restart {
                0: RestartArguments { arguments: None }
            }
        ));

        // Restarting a launch request
        let req = simulate_poll_request(
            "Content-Length: 96\r\n\r\n{\"seq\": 152,\"type\": \"request\",\"command\": \"restart\",\"arguments\": {\"arguments\": {\"noDebug\":true}}}",
        );
        assert!(matches!(
            req.command,
            Command::Restart {
                0: RestartArguments {
                    arguments: Some(AttachOrLaunchArguments {
                        no_debug: Some(_),
                        ..
                    })
                }
            }
        ));

        // Restarting a launch or attach request
        let req = simulate_poll_request(
            "Content-Length: 98\r\n\r\n{\"seq\": 152,\"type\": \"request\",\"command\": \"restart\",\"arguments\": {\"arguments\": {\"__restart\":true}}}",
        );
        assert!(matches!(
            req.command,
            Command::Restart {
                0: RestartArguments {
                    arguments: Some(AttachOrLaunchArguments {
                        restart_data: Some(Value::Bool(true)),
                        ..
                    })
                }
            }
        ));
    }
}
