//! JSON-RPC client used to talk to one MCP server.

pub mod client;
pub mod pending;

pub use client::{DEFAULT_REQUEST_TIMEOUT, McpClient, McpClientError};
pub use pending::PendingRequests;

#[cfg(test)]
mod tests;
