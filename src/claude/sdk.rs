//! Wrapper around the `claude` CLI subprocess for Agent SDK communication.
//!
//! The Claude Agent SDK spawns `claude` as a subprocess and communicates
//! via JSON over stdin/stdout. This module replicates that protocol in Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Messages received from the Claude CLI subprocess.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SdkMessage {
    #[serde(rename = "system")]
    System {
        #[serde(default)]
        subtype: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        content: Vec<ContentBlock>,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    },
    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        usage: Option<UsageInfo>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_name: String,
        #[serde(default)]
        input: Value,
        request_id: String,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// Message content for the SDK user message protocol.
#[derive(Debug, Serialize)]
pub struct SdkUserMessageContent {
    pub role: String,
    pub content: String,
}

/// Commands sent to the Claude CLI subprocess.
/// Matches the Agent SDK's streamInput protocol.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum SdkCommand {
    /// User message — matches SDK's SDKUserMessage format.
    #[serde(rename = "user")]
    UserMessage {
        message: SdkUserMessageContent,
        parent_tool_use_id: Option<String>,
    },
    /// Tool result (approval/denial).
    #[serde(rename = "tool_result")]
    ToolResult {
        request_id: String,
        behavior: String, // "allow" or "deny"
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
    },
    /// Control command for SDK metadata queries.
    #[serde(rename = "control")]
    Control {
        command: String,
        #[serde(flatten)]
        params: HashMap<String, Value>,
    },
}

/// A running Claude CLI session.
pub struct ClaudeProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    pub message_rx: mpsc::UnboundedReceiver<SdkMessage>,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl ClaudeProcess {
    /// Spawn a new `claude` CLI process with the Agent SDK protocol.
    pub async fn spawn(
        cwd: &Path,
        session_id: Option<&str>,
        model: Option<&str>,
        mcp_config_path: Option<&Path>,
    ) -> Result<Self, String> {
        let mut cmd = tokio::process::Command::new("claude");
        cmd.arg("--output-format=stream-json")
            .arg("--verbose")
            .arg("--input-format=stream-json");

        if let Some(sid) = session_id {
            cmd.arg("--resume").arg(sid);
        }

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        if let Some(mcp_path) = mcp_config_path {
            cmd.arg("--mcp-config").arg(mcp_path);
        }

        cmd.current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            format!("Failed to spawn claude CLI: {e}. Is claude installed? Run: npm install -g @anthropic-ai/claude-code")
        })?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn stderr reader for logging
        let stderr_reader = BufReader::new(stderr);
        tokio::spawn(async move {
            let mut lines = stderr_reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!("[claude stderr] {line}");
            }
        });

        // Spawn stdout reader that parses JSON messages
        let reader_handle = tokio::spawn(Self::read_messages(stdout, tx));

        Ok(Self {
            child,
            stdin: Some(stdin),
            message_rx: rx,
            _reader_handle: reader_handle,
        })
    }

    async fn read_messages(stdout: ChildStdout, tx: mpsc::UnboundedSender<SdkMessage>) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<SdkMessage>(&line) {
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        break; // receiver dropped
                    }
                }
                Err(e) => {
                    warn!("[claude] Failed to parse message: {e} — line: {line}");
                }
            }
        }
    }

    /// Send a user message to the Claude process using SDK protocol format.
    pub async fn send_message(&mut self, message: &str) -> Result<(), String> {
        let cmd = SdkCommand::UserMessage {
            message: SdkUserMessageContent {
                role: "user".to_string(),
                content: message.to_string(),
            },
            parent_tool_use_id: None,
        };
        self.send_command(&cmd).await
    }

    /// Send a tool result (approval/denial) to the Claude process.
    pub async fn send_tool_result(
        &mut self,
        request_id: &str,
        behavior: &str,
        message: Option<&str>,
        updated_input: Option<Value>,
    ) -> Result<(), String> {
        let cmd = SdkCommand::ToolResult {
            request_id: request_id.to_string(),
            behavior: behavior.to_string(),
            message: message.map(|s| s.to_string()),
            updated_input,
        };
        self.send_command(&cmd).await
    }

    /// Send a control command.
    pub async fn send_control(
        &mut self,
        command: &str,
        params: HashMap<String, Value>,
    ) -> Result<(), String> {
        let cmd = SdkCommand::Control {
            command: command.to_string(),
            params,
        };
        self.send_command(&cmd).await
    }

    async fn send_command(&mut self, cmd: &SdkCommand) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "stdin closed".to_string())?;
        let json = serde_json::to_string(cmd).map_err(|e| format!("JSON serialize error: {e}"))?;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to claude stdin: {e}"))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to write newline: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush stdin: {e}"))?;
        Ok(())
    }

    /// Close the subprocess.
    pub async fn close(&mut self) {
        // Drop stdin to signal EOF (safe — Option::take)
        self.stdin.take();
        // Try to kill if still running
        let _ = self.child.kill().await;
    }

    /// Check if the process is still running.
    pub fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        // Best-effort kill
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SdkMessage deserialization ---

    #[test]
    fn parse_system_init_message() {
        let json = r#"{"type":"system","subtype":"init","session_id":"sess-123"}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::System {
                subtype,
                session_id,
                ..
            } => {
                assert_eq!(subtype, "init");
                assert_eq!(session_id.unwrap(), "sess-123");
            }
            _ => panic!("Expected System message"),
        }
    }

    #[test]
    fn parse_assistant_message_with_text() {
        let json = r#"{"type":"assistant","content":[{"type":"text","text":"Hello world"}]}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::Assistant { content, .. } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
                    _ => panic!("Expected Text block"),
                }
            }
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn parse_assistant_message_empty_content() {
        let json = r#"{"type":"assistant","content":[]}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::Assistant { content, .. } => assert!(content.is_empty()),
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn parse_tool_use_message() {
        let json = r#"{"type":"tool_use","tool_name":"Bash","input":{"command":"ls"},"request_id":"req-1"}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::ToolUse {
                tool_name,
                input,
                request_id,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(input["command"], "ls");
                assert_eq!(request_id, "req-1");
            }
            _ => panic!("Expected ToolUse message"),
        }
    }

    #[test]
    fn parse_result_message_with_usage() {
        let json = r#"{
            "type": "result",
            "result": "Task done",
            "duration_ms": 5000,
            "usage": {"input_tokens": 1000, "output_tokens": 500},
            "total_cost_usd": 0.05
        }"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::Result {
                result,
                duration_ms,
                usage,
                total_cost_usd,
                ..
            } => {
                assert_eq!(result.unwrap(), "Task done");
                assert_eq!(duration_ms.unwrap(), 5000);
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 1000);
                assert_eq!(u.output_tokens, 500);
                assert!((total_cost_usd.unwrap() - 0.05).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Result message"),
        }
    }

    #[test]
    fn parse_result_message_minimal() {
        let json = r#"{"type":"result"}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::Result {
                result,
                duration_ms,
                usage,
                total_cost_usd,
                ..
            } => {
                assert!(result.is_none());
                assert!(duration_ms.is_none());
                assert!(usage.is_none());
                assert!(total_cost_usd.is_none());
            }
            _ => panic!("Expected Result message"),
        }
    }

    #[test]
    fn parse_unknown_message_type() {
        let json = r#"{"type":"some_future_type","data":"whatever"}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, SdkMessage::Unknown));
    }

    #[test]
    fn parse_system_with_extra_fields() {
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","tools":["Bash","Read"]}"#;
        let msg: SdkMessage = serde_json::from_str(json).unwrap();
        match msg {
            SdkMessage::System { extra, .. } => {
                assert!(extra.contains_key("tools"));
            }
            _ => panic!("Expected System message"),
        }
    }

    // --- SdkCommand serialization ---

    #[test]
    fn serialize_user_message() {
        let cmd = SdkCommand::UserMessage {
            message: SdkUserMessageContent {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
            parent_tool_use_id: None,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "user");
        assert_eq!(json["message"]["role"], "user");
        assert_eq!(json["message"]["content"], "Hello");
        assert!(json["parent_tool_use_id"].is_null());
    }

    #[test]
    fn serialize_tool_result_allow() {
        let cmd = SdkCommand::ToolResult {
            request_id: "req-1".to_string(),
            behavior: "allow".to_string(),
            message: None,
            updated_input: None,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["behavior"], "allow");
        // message and updated_input should be absent (skip_serializing_if)
        assert!(!json.as_object().unwrap().contains_key("message"));
        assert!(!json.as_object().unwrap().contains_key("updated_input"));
    }

    #[test]
    fn serialize_tool_result_deny_with_message() {
        let cmd = SdkCommand::ToolResult {
            request_id: "req-2".to_string(),
            behavior: "deny".to_string(),
            message: Some("Not allowed".to_string()),
            updated_input: None,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["behavior"], "deny");
        assert_eq!(json["message"], "Not allowed");
    }

    #[test]
    fn serialize_control_command() {
        let mut params = HashMap::new();
        params.insert(
            "server_name".to_string(),
            Value::String("mcp-server".to_string()),
        );
        let cmd = SdkCommand::Control {
            command: "toggle_mcp_server".to_string(),
            params,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "control");
        assert_eq!(json["command"], "toggle_mcp_server");
        assert_eq!(json["server_name"], "mcp-server");
    }

    // --- ContentBlock ---

    #[test]
    fn parse_text_content_block() {
        let json = r#"{"type":"text","text":"hello"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn parse_unknown_content_block() {
        let json = r#"{"type":"image","url":"http://example.com"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(matches!(block, ContentBlock::Other));
    }
}
