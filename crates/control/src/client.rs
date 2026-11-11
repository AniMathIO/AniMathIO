//! Blocking client for the control socket, used by the MCP sidecar and tests.

use std::io::{self, Write};

use interprocess::local_socket::traits::Stream as _;
use serde_json::{Value, json};

use crate::{
    Call, ControlError, ControlHandler,
    server::{MAX_RESPONSE_BYTES, SocketSpec, default_socket_spec, read_message},
    token,
};

/// A client connected to a running control server.
pub struct ControlClient {
    stream: interprocess::local_socket::Stream,
    token: String,
    next_id: u64,
}

impl ControlClient {
    /// Connect to the socket described by `spec` and read its token file.
    pub fn connect(spec: &SocketSpec) -> io::Result<Self> {
        let name = crate::server::local_name(&spec.path)?;
        let stream = interprocess::local_socket::Stream::connect(name)?;
        let token = token::read_file(&spec.token_file)?;
        Ok(Self {
            stream,
            token,
            next_id: 1,
        })
    }

    /// Connect to the well-known per-user control socket, if present.
    pub fn connect_default() -> io::Result<Self> {
        Self::connect(&default_socket_spec()?)
    }

    /// Connect with an explicit token override, for clients that already hold
    /// the token out-of-band and for auth-rejection tests.
    pub fn connect_with_token(spec: &SocketSpec, token: impl Into<String>) -> io::Result<Self> {
        let mut client = Self::connect(spec)?;
        client.token = token.into();
        Ok(client)
    }

    /// Send one call and return its JSON result.
    pub fn call(&mut self, call: Call) -> Result<Value, ControlError> {
        let request = self.build_request(call)?;
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        self.stream.write_all(&bytes)?;
        self.stream.flush()?;
        let response = read_message(&mut self.stream, MAX_RESPONSE_BYTES)?.ok_or_else(|| {
            ControlError::Protocol("control server closed the connection".to_owned())
        })?;
        let response: Value = serde_json::from_slice(&response)?;
        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("control error");
            return Err(match error.get("code").and_then(Value::as_i64) {
                Some(-32_001) => ControlError::Unauthorized,
                Some(-32_601) => ControlError::MethodNotFound(message.to_owned()),
                Some(-32_602) => ControlError::InvalidParams(message.to_owned()),
                Some(-32_002) => ControlError::Unsupported(message.to_owned()),
                _ => ControlError::Protocol(message.to_owned()),
            });
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    fn build_request(&mut self, call: Call) -> Result<Value, ControlError> {
        let call = serde_json::to_value(call)?;
        let method = call.get("method").cloned().unwrap_or(Value::Null);
        let params = call.get("params").cloned().unwrap_or(Value::Null);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
            "token": self.token,
        }))
    }
}

impl ControlHandler for ControlClient {
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        self.call(call)
    }
}
