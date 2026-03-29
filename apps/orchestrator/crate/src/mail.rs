use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::error::{OrchestratorError, Result};

/// An incoming message from the Agent Mail inbox.
#[derive(Debug, Clone, Deserialize)]
pub struct InboxMessage {
    pub id: Option<i64>,
    pub topic: Option<String>,
    pub subject: Option<String>,
    pub sender_name: Option<String>,
    #[serde(alias = "from")]
    pub from_name: Option<String>,
    pub body_md: Option<String>,
    pub body: Option<String>,
    pub thread_id: Option<String>,
    pub ack_required: Option<bool>,
    pub read_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub acked_at: Option<String>,
}

impl InboxMessage {
    /// Get the sender name from whichever field is available.
    pub fn sender(&self) -> &str {
        self.sender_name
            .as_deref()
            .or(self.from_name.as_deref())
            .unwrap_or("")
    }

    /// Get the message body from whichever field is available.
    pub fn body_text(&self) -> &str {
        self.body_md
            .as_deref()
            .or(self.body.as_deref())
            .unwrap_or("")
    }

    /// Whether this message has been acknowledged.
    pub fn is_acknowledged(&self) -> bool {
        self.acknowledged_at.is_some() || self.acked_at.is_some()
    }
}

/// An outgoing message to send via Agent Mail.
#[derive(Debug, Clone, Serialize)]
pub struct OutgoingMessage {
    pub project_key: String,
    pub sender_name: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body_md: String,
    pub importance: String,
    pub ack_required: bool,
    pub topic: String,
    pub thread_id: String,
}

/// Agent Mail HTTP client.
pub struct MailClient {
    url: String,
    token: Option<String>,
    agent: ureq::Agent,
    max_retries: u32,
}

impl MailClient {
    pub fn new(config: &MailConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(config.connect_timeout_secs))
            .timeout_read(Duration::from_secs(config.max_time_secs))
            // Disable connection pooling to prevent CLOSE_WAIT sockets.
            // When Agent Mail restarts, pooled connections go stale and cause
            // "Connection reset by peer" errors. Fresh connections per request
            // are fine at our 8s poll interval.
            .max_idle_connections(0)
            .build();
        Self {
            url: config.url.clone(),
            token: config.token.clone(),
            agent,
            max_retries: config.retry_attempts,
        }
    }

    /// Send a JSON-RPC 2.0 request to the Agent Mail MCP server.
    pub fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        });

        let body_str = serde_json::to_string(&request_body)?;

        let mut req = self.agent
            .post(&self.url)
            .set("Content-Type", "application/json");

        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }

        let response = req
            .send_string(&body_str)
            .map_err(|e| OrchestratorError::Http(format!("request to {}: {e}", self.url)))?;

        let response_body = response
            .into_string()
            .map_err(|e| OrchestratorError::Http(format!("reading response: {e}")))?;

        let parsed: serde_json::Value = serde_json::from_str(&response_body)?;

        // Check for MCP error response
        if let Some(result) = parsed.get("result") {
            if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
                let error_text = result
                    .get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown MCP error");
                return Err(OrchestratorError::Mail(error_text.to_string()));
            }
        }

        Ok(parsed)
    }

    /// Call a tool with retry logic for mutating operations.
    fn call_tool_with_retry(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut sleep_secs = 1u64;
        let max_sleep = 16u64;

        for attempt in 0..self.max_retries {
            match self.call_tool(name, arguments.clone()) {
                Ok(result) => return Ok(result),
                Err(OrchestratorError::Http(_)) if attempt < self.max_retries - 1 => {
                    tracing::warn!(
                        tool = name,
                        attempt = attempt + 1,
                        max = self.max_retries,
                        "retrying after HTTP error"
                    );
                    thread::sleep(Duration::from_secs(sleep_secs));
                    sleep_secs = (sleep_secs * 2).min(max_sleep);
                }
                Err(e) => return Err(e),
            }
        }

        Err(OrchestratorError::Http("max retries exhausted without result".into()))
    }

    /// Fetch the coordinator's inbox.
    pub fn fetch_inbox(
        &self,
        project_key: &str,
        agent_name: &str,
        limit: usize,
    ) -> Result<Vec<InboxMessage>> {
        let args = serde_json::json!({
            "project_key": project_key,
            "agent_name": agent_name,
            "limit": limit,
            "include_bodies": true,
        });

        let response = self.call_tool("fetch_inbox", args)?;
        extract_messages(&response)
    }

    /// Send a message via Agent Mail (with retry).
    pub fn send_message(&self, msg: &OutgoingMessage) -> Result<()> {
        let args = serde_json::to_value(msg)?;
        self.call_tool_with_retry("send_message", args)?;
        Ok(())
    }

    /// Send a message via Agent Mail (single attempt, no retry).
    /// Use for non-critical sends where blocking on retries is worse than dropping the message.
    pub fn send_message_best_effort(&self, msg: &OutgoingMessage) -> Result<()> {
        let args = serde_json::to_value(msg)?;
        self.call_tool("send_message", args)?;
        Ok(())
    }

    /// Acknowledge a message (single attempt — acks are idempotent).
    pub fn acknowledge(
        &self,
        msg_id: i64,
        project_key: &str,
        agent_name: &str,
    ) -> Result<()> {
        let args = serde_json::json!({
            "message_id": msg_id,
            "project_key": project_key,
            "agent_name": agent_name,
        });
        self.call_tool("acknowledge_message", args)?;
        Ok(())
    }

    /// Mark a message as read.
    pub fn mark_read(
        &self,
        msg_id: i64,
        project_key: &str,
        agent_name: &str,
    ) -> Result<()> {
        let args = serde_json::json!({
            "message_id": msg_id,
            "project_key": project_key,
            "agent_name": agent_name,
        });
        self.call_tool("mark_message_read", args)?;
        Ok(())
    }
}

/// Extract messages from the nested MCP JSON-RPC response.
fn extract_messages(response: &serde_json::Value) -> Result<Vec<InboxMessage>> {
    // Navigate: response.result.content[].text -> parse as JSON -> get "messages"
    let result = response.get("result").unwrap_or(response);

    // If the result itself has "messages" directly
    if let Some(messages) = result.get("messages") {
        if let Ok(msgs) = serde_json::from_value::<Vec<InboxMessage>>(messages.clone()) {
            return Ok(msgs);
        }
    }

    // Navigate through content array
    let content = match result.get("content") {
        Some(c) => c,
        None => {
            if let Ok(msgs) = serde_json::from_value::<Vec<InboxMessage>>(result.clone()) {
                return Ok(msgs);
            }
            return Ok(Vec::new());
        }
    };

    let content_array = match content.as_array() {
        Some(arr) => arr,
        None => return Ok(Vec::new()),
    };

    for item in content_array {
        if item.get("type").and_then(|t| t.as_str()) != Some("text") {
            continue;
        }
        let text = match item.get("text").and_then(|t| t.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let parsed: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(messages) = parsed.get("messages") {
            if let Ok(msgs) = serde_json::from_value::<Vec<InboxMessage>>(messages.clone()) {
                return Ok(msgs);
            }
        }
        if let Ok(msgs) = serde_json::from_value::<Vec<InboxMessage>>(parsed) {
            return Ok(msgs);
        }
    }

    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_messages_direct() {
        let json = serde_json::json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"messages\": [{\"id\": 1, \"subject\": \"test\", \"topic\": \"bead-complete\"}]}"
                }]
            }
        });
        let msgs = extract_messages(&json).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, Some(1));
    }

    #[test]
    fn test_extract_messages_empty() {
        let json = serde_json::json!({});
        let msgs = extract_messages(&json).unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_inbox_message_sender() {
        let msg = InboxMessage {
            id: Some(1),
            topic: None,
            subject: Some("test".into()),
            sender_name: Some("Worker1".into()),
            from_name: None,
            body_md: Some("body".into()),
            body: None,
            thread_id: None,
            ack_required: None,
            read_at: None,
            acknowledged_at: None,
            acked_at: None,
        };
        assert_eq!(msg.sender(), "Worker1");
        assert_eq!(msg.body_text(), "body");
        assert!(!msg.is_acknowledged());
    }

    #[test]
    fn test_inbox_message_acknowledged() {
        let msg = InboxMessage {
            id: Some(1),
            topic: None,
            subject: None,
            sender_name: None,
            from_name: None,
            body_md: None,
            body: None,
            thread_id: None,
            ack_required: None,
            read_at: None,
            acknowledged_at: Some("2026-01-01T00:00:00Z".into()),
            acked_at: None,
        };
        assert!(msg.is_acknowledged());
    }

    #[test]
    fn test_outgoing_message_serializes() {
        let msg = OutgoingMessage {
            project_key: "/tmp/project".into(),
            sender_name: "Coordinator".into(),
            to: vec!["Worker1".into()],
            subject: "[pat-abc] Assigned".into(),
            body_md: "Do the thing".into(),
            importance: "normal".into(),
            ack_required: true,
            topic: "bead-assign".into(),
            thread_id: "pat-abc".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["to"][0], "Worker1");
        assert_eq!(json["ack_required"], true);
    }
}
