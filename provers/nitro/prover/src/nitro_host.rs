use anyhow::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use vsock::{VsockAddr, VsockListener, VsockStream};

use nitro_common::{Command, LogMessage, Response};

// Host
// todo: a lisner for client to report/send back sth
pub struct HostConnection {
    cmd_stream: VsockStream,
}

impl HostConnection {
    pub fn connect(enclave_cid: u32, cmd_port: u32) -> Result<Self, Error> {
        let cmd_stream =
            VsockStream::connect(&VsockAddr::new(enclave_cid, cmd_port)).map_err(|e| {
                eprintln!("Error connecting to command stream: {}", e);
                Error::msg(e.to_string())
            })?;

        Ok(Self { cmd_stream })
    }

    pub fn listen(cid: u32, cmd_port: u32) -> Result<Self, Error> {
        let cmd_listener = VsockListener::bind(&VsockAddr::new(cid, cmd_port)).map_err(|e| {
            eprintln!("Error connecting to command stream: {}", e);
            Error::msg(e.to_string())
        })?;
        println!("Listening for command connection on port {}", cmd_port);

        let (cmd_stream, _addr) = cmd_listener.accept()?;
        println!("Command connection established");

        Ok(Self { cmd_stream })
    }

    pub fn send_command(&mut self, command: Command) -> Result<Response, Error> {
        let cmd_bytes = serde_json::to_vec(&command)?;
        self.cmd_stream.write_all(&cmd_bytes)?;

        let mut buf = vec![0; 1024];
        let n = self.cmd_stream.read(&mut buf)?;
        let response: Response = serde_json::from_slice(&buf[..n])?;

        Ok(response)
    }
}
