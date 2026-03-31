use crate::claude::output_formatter::*;
use crate::claude::sdk::{ClaudeProcess, ContentBlock, SdkMessage};
use crate::config::get_config;
use crate::db::Database;
use crate::db::types::SessionStatus;
use crate::discord::DiscordClient;
use parking_lot::Mutex;
use serenity::all::{
    ChannelId, CreateAttachment, CreateEmbed, CreateMessage, EditMessage, GuildId,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, oneshot};
use tracing::{error, warn};
use uuid::Uuid;

type ApprovalDecision = (String, Option<String>, Option<serde_json::Value>);
// (behavior, message, updated_input)

struct PendingApproval {
    tx: oneshot::Sender<ApprovalDecision>,
    channel_id: String,
}

struct PendingQuestion {
    tx: oneshot::Sender<Option<String>>,
    channel_id: String,
}

struct ActiveSession {
    process: ClaudeProcess,
    session_id: Option<String>,
    db_id: String,
    busy: bool,
}

struct QueueItem {
    channel_id: ChannelId,
    guild_id: GuildId,
    prompt: String,
}

pub struct SessionManager {
    db: Database,
    discord: Arc<dyn DiscordClient>,
    sessions: RwLock<HashMap<String, Arc<tokio::sync::Mutex<ActiveSession>>>>,
    pending_approvals: Arc<Mutex<HashMap<String, PendingApproval>>>,
    pending_questions: Arc<Mutex<HashMap<String, PendingQuestion>>>,
    pending_custom_inputs: Arc<Mutex<HashMap<String, String>>>, // channel_id -> request_id
    message_queues: Arc<Mutex<HashMap<String, Vec<QueueItem>>>>,
    pending_queue_prompts: Arc<Mutex<HashMap<String, QueueItem>>>,
    // Operational tuning (set from config at construction time)
    edit_interval_ms: u128,
    approval_timeout_secs: u64,
    max_queue_size: usize,
    sdk_call_timeout_secs: u64,
}

impl SessionManager {
    pub fn new(db: Database, discord: Arc<dyn DiscordClient>) -> Arc<Self> {
        let config = get_config();
        Self::new_with_settings(
            db,
            discord,
            config.edit_interval_ms,
            config.approval_timeout_secs,
            config.max_queue_size,
            config.sdk_call_timeout_secs,
        )
    }

    /// Constructor with explicit settings (useful for tests without global config).
    pub fn new_with_settings(
        db: Database,
        discord: Arc<dyn DiscordClient>,
        edit_interval_ms: u128,
        approval_timeout_secs: u64,
        max_queue_size: usize,
        sdk_call_timeout_secs: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            discord,
            sessions: RwLock::new(HashMap::new()),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            pending_questions: Arc::new(Mutex::new(HashMap::new())),
            pending_custom_inputs: Arc::new(Mutex::new(HashMap::new())),
            message_queues: Arc::new(Mutex::new(HashMap::new())),
            pending_queue_prompts: Arc::new(Mutex::new(HashMap::new())),
            edit_interval_ms,
            approval_timeout_secs,
            max_queue_size,
            sdk_call_timeout_secs,
        })
    }

    pub fn send_message(
        self: &Arc<Self>,
        channel_id: ChannelId,
        guild_id: GuildId,
        prompt: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let prompt = prompt.to_string();
        Box::pin(async move {
            let prompt = &prompt;
            let channel_id_str = channel_id.to_string();
            let config = get_config();

            // Auto-register channel if not registered
            let project = match self.db.get_project(&channel_id_str) {
                Some(p) => p,
                None => {
                    let project_path = config.sessions_dir().join(&channel_id_str);
                    tokio::fs::create_dir_all(&project_path).await.ok();
                    if let Err(e) = self.db.register_project(
                        &channel_id_str,
                        &project_path.to_string_lossy(),
                        &guild_id.to_string(),
                    ) {
                        warn!(channel_id = %channel_id_str, error = %e, "failed to register project");
                    }
                    self.db.get_project(&channel_id_str)
                    .ok_or_else(|| format!("Failed to retrieve project after registration for channel {channel_id_str}"))?
                }
            };

            // Check if session exists; create if needed
            let session_arc = match self.ensure_session(channel_id, &project.project_path).await {
                Ok(s) => s,
                Err(e) => {
                    // #30: If stale session, clear and retry
                    if e.contains("No conversation found with session ID") {
                        warn!(channel_id = %channel_id_str, "stale session detected, clearing and retrying fresh");
                        if let Err(e) = self.db.clear_session(&channel_id_str) {
                            warn!(channel_id = %channel_id_str, error = %e, "failed to clear session");
                        }
                        self.cleanup_session_internal(&channel_id_str).await;
                        return self.send_message(channel_id, guild_id, prompt).await;
                    }
                    return Err(e);
                }
            };

            // Set up the "Thinking..." message with stop button
            let stop_row = create_stop_button(&channel_id_str);
            let msg = self
                .discord
                .send_message(
                    channel_id,
                    CreateMessage::new()
                        .content("⏳ Thinking...")
                        .components(vec![stop_row.clone()]),
                )
                .await
                .map_err(|e| format!("Failed to send thinking message: {e}"))?;

            let msg_id = msg;
            let start_time = Instant::now();

            // Send the user message to the Claude process
            {
                let mut session = session_arc.lock().await;
                session.busy = true;
                session.process.send_message(prompt).await?;
            }

            self.update_status(&channel_id_str, SessionStatus::Online);

            // Process messages from the Claude process
            let mut response_buffer = String::new();
            let mut last_edit_time = Instant::now() - std::time::Duration::from_secs(10);
            let mut current_msg_id = msg_id;
            let mut tool_use_count = 0u32;
            let mut has_text_output = false;
            let mut has_result = false;
            let mut last_activity = "Thinking...".to_string();

            loop {
                let message = {
                    let mut session = session_arc.lock().await;
                    tokio::select! {
                        msg = session.process.message_rx.recv() => msg,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                            // Heartbeat — update status message if no text output yet
                            if !has_text_output {
                                let elapsed = start_time.elapsed().as_secs();
                                let time_str = if elapsed > 60 {
                                    format!("{}m {}s", elapsed / 60, elapsed % 60)
                                } else {
                                    format!("{elapsed}s")
                                };
                                let _ = self.discord.edit_message(
                                    channel_id,
                                    current_msg_id,
                                    EditMessage::new()
                                        .content(format!("⏳ {last_activity} ({time_str})"))
                                        .components(vec![stop_row.clone()]),
                                ).await;
                            }
                            continue;
                        }
                    }
                };

                let message = match message {
                    Some(m) => m,
                    None => break, // Process ended
                };

                match message {
                    SdkMessage::System {
                        subtype,
                        session_id,
                        ..
                    } => {
                        if subtype == "init"
                            && let Some(sid) = session_id
                        {
                            let mut session = session_arc.lock().await;
                            session.session_id = Some(sid.clone());
                            if let Err(e) = self.db.upsert_session(
                                &session.db_id,
                                &channel_id_str,
                                Some(&sid),
                                SessionStatus::Idle,
                            ) {
                                warn!("Failed to upsert session: {e}");
                            }

                            // Apply per-channel disabled MCPs
                            let disabled = self.db.get_disabled_mcps(&channel_id_str);
                            for name in disabled {
                                let mut params = HashMap::new();
                                params.insert(
                                    "server_name".to_string(),
                                    serde_json::Value::String(name),
                                );
                                params
                                    .insert("enabled".to_string(), serde_json::Value::Bool(false));
                                let _ = session
                                    .process
                                    .send_control("toggle_mcp_server", params)
                                    .await;
                            }
                        }
                    }

                    SdkMessage::Assistant { content, .. } => {
                        for block in &content {
                            if let ContentBlock::Text { text } = block {
                                response_buffer.push_str(text);
                                has_text_output = true;
                            }
                        }

                        // Throttled message edit
                        let now = Instant::now();
                        if now.duration_since(last_edit_time).as_millis() >= self.edit_interval_ms
                            && !response_buffer.is_empty()
                        {
                            last_edit_time = now;
                            let chunks = split_message(&response_buffer);
                            if let Some(first) = chunks.first() {
                                let _ = self
                                    .discord
                                    .edit_message(
                                        channel_id,
                                        current_msg_id,
                                        EditMessage::new().content(first).components(vec![]),
                                    )
                                    .await;
                            }
                            for chunk in chunks.iter().skip(1) {
                                if let Ok(new_id) = self
                                    .discord
                                    .send_message(channel_id, CreateMessage::new().content(chunk))
                                    .await
                                {
                                    current_msg_id = new_id;
                                }
                            }
                            // Clear buffer only after all chunks sent
                            if chunks.len() > 1 {
                                response_buffer.clear();
                            }
                        }
                    }

                    SdkMessage::ToolUse {
                        tool_name,
                        input,
                        request_id,
                        ..
                    } => {
                        tool_use_count += 1;

                        let tool_label = match tool_name.as_str() {
                            "Read" => "Reading files",
                            "Glob" => "Searching files",
                            "Grep" => "Searching code",
                            "Write" => "Writing file",
                            "Edit" => "Editing file",
                            "Bash" => "Running command",
                            "WebSearch" => "Searching web",
                            "WebFetch" => "Fetching URL",
                            "TodoWrite" => "Updating tasks",
                            "mcp__user__send_file" => "Sending file",
                            other => other,
                        };
                        let file_hint = input
                            .get("file_path")
                            .and_then(|v| v.as_str())
                            .and_then(|p| p.rsplit(&['/', '\\'][..]).next())
                            .map(|f| format!(" `{f}`"))
                            .unwrap_or_default();
                        // Escape underscores in fallback tool names to prevent Discord italics
                        let escaped_label = tool_label.replace('_', "\\_");
                        last_activity = format!("{escaped_label}{file_hint}");

                        if !has_text_output {
                            let elapsed = start_time.elapsed().as_secs();
                            let time_str = if elapsed > 60 {
                                format!("{}m {}s", elapsed / 60, elapsed % 60)
                            } else {
                                format!("{elapsed}s")
                            };
                            let _ = self.discord
                            .edit_message(
                                channel_id,
                                current_msg_id,
                                EditMessage::new()
                                    .content(format!(
                                        "⏳ {last_activity} ({time_str}) [{tool_use_count} tools used]"
                                    ))
                                    .components(vec![stop_row.clone()]),
                            )
                            .await;
                        }

                        let decision = self
                            .handle_tool_use(
                                channel_id,
                                &channel_id_str,
                                &tool_name,
                                &input,
                                &request_id,
                            )
                            .await;

                        let mut session = session_arc.lock().await;
                        let _ = session
                            .process
                            .send_tool_result(
                                &request_id,
                                &decision.0,
                                decision.1.as_deref(),
                                decision.2,
                            )
                            .await;
                    }

                    SdkMessage::Result {
                        result,
                        duration_ms,
                        usage,
                        total_cost_usd,
                        ..
                    } => {
                        // Flush remaining buffer
                        if !response_buffer.is_empty() {
                            let chunks = split_message(&response_buffer);
                            if let Some(first) = chunks.first() {
                                let _ = self
                                    .discord
                                    .edit_message(
                                        channel_id,
                                        current_msg_id,
                                        EditMessage::new().content(first),
                                    )
                                    .await;
                            }
                            for chunk in chunks.iter().skip(1) {
                                let _ = self
                                    .discord
                                    .send_message(channel_id, CreateMessage::new().content(chunk))
                                    .await;
                            }
                        }

                        // Replace stop button with completed button
                        let _ = self
                            .discord
                            .edit_message(
                                channel_id,
                                current_msg_id,
                                EditMessage::new().components(vec![create_completed_button()]),
                            )
                            .await;

                        let result_text = result.as_deref().unwrap_or("Task completed");
                        let (embed, file_data) = create_result_embed(
                            result_text,
                            usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                            usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                            duration_ms.unwrap_or(0),
                            total_cost_usd,
                        );
                        let mut create_msg = CreateMessage::new().embed(embed);
                        if let Some(data) = file_data {
                            create_msg =
                                create_msg.add_file(CreateAttachment::bytes(data, "result.txt"));
                        }
                        let _ = self.discord.send_message(channel_id, create_msg).await;

                        // Detect auth/credit errors
                        let lower_result = result_text.to_lowercase();
                        let auth_keywords = [
                            "credit balance",
                            "not authenticated",
                            "unauthorized",
                            "authentication",
                            "login required",
                            "auth token",
                            "expired",
                            "not logged in",
                            "please run /login",
                        ];
                        if auth_keywords.iter().any(|kw| lower_result.contains(kw)) {
                            let _ = self.discord
                            .send_message(
                                channel_id,
                                CreateMessage::new().content(
                                    "🔑 Claude Code is not logged in. Please open a terminal on the host PC and run `claude login` to authenticate, then try again.",
                                ),
                            )
                            .await;
                        }

                        has_result = true;
                        {
                            let mut session = session_arc.lock().await;
                            session.busy = false;
                        }
                        self.update_status(&channel_id_str, SessionStatus::Idle);

                        // #1: Process next queued message (actually calls send_message)
                        self.process_queue(&channel_id_str).await;
                        break;
                    }

                    SdkMessage::Unknown => {}
                }
            }

            // #14: If we exited without a result, handle error properly
            if !has_result {
                self.handle_turn_error(channel_id, &channel_id_str, &session_arc, &start_time)
                    .await;
            }

            Ok(())
        }) // Box::pin
    }

    /// #14: Handle errors that occur during a turn — parse API errors, detect auth issues.
    async fn handle_turn_error(
        &self,
        channel_id: ChannelId,
        channel_id_str: &str,
        session_arc: &Arc<tokio::sync::Mutex<ActiveSession>>,
        _start_time: &Instant,
    ) {
        {
            let mut session = session_arc.lock().await;
            session.busy = false;
        }

        // Try to get exit status for error info
        let exit_info = {
            let mut session = session_arc.lock().await;
            session.process.try_wait()
        };

        let mut err_msg = match exit_info {
            Some(status) => format!(
                "Claude process exited with {status}. The server may be temporarily unavailable — please try again later."
            ),
            None => "Claude session ended unexpectedly.".to_string(),
        };

        // Detect auth/credit errors
        let auth_keywords = [
            "credit balance",
            "not authenticated",
            "unauthorized",
            "authentication",
            "login required",
            "auth token",
            "expired",
            "not logged in",
            "please run /login",
        ];
        let lower_msg = err_msg.to_lowercase();
        if auth_keywords.iter().any(|kw| lower_msg.contains(kw)) {
            err_msg.push_str(
                "\n\n🔑 Claude Code is not logged in. Please open a terminal on the host PC and run `claude login` to authenticate, then try again.",
            );
        }

        let _ = self
            .discord
            .send_message(
                channel_id,
                CreateMessage::new().content(format!("❌ {err_msg}")),
            )
            .await;

        self.update_status(channel_id_str, SessionStatus::Offline);
        self.sessions.write().await.remove(channel_id_str);
        self.cleanup_pending(channel_id_str);
    }

    async fn ensure_session(
        &self,
        channel_id: ChannelId,
        project_path: &str,
    ) -> Result<Arc<tokio::sync::Mutex<ActiveSession>>, String> {
        let channel_id_str = channel_id.to_string();

        // Return existing session
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&channel_id_str) {
                return Ok(session.clone());
            }
        }

        // Check DB for previous session_id (for resume after restart)
        let db_session = self.db.get_session(&channel_id_str);
        let db_id = db_session
            .as_ref()
            .map(|s| s.db_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let resume_session_id = db_session.and_then(|s| s.claude_session_id);

        let model = self.db.get_model(&channel_id_str);
        let project_path_buf = PathBuf::from(project_path);

        // Check for .mcp.json in project dir
        let mcp_config = project_path_buf.join(".mcp.json");
        let mcp_config_ref = if mcp_config.exists() {
            Some(mcp_config.as_path())
        } else {
            None
        };

        let process = ClaudeProcess::spawn(
            &project_path_buf,
            resume_session_id.as_deref(),
            model.as_deref(),
            mcp_config_ref,
        )
        .await?;

        let session = ActiveSession {
            process,
            session_id: resume_session_id,
            db_id: db_id.clone(),
            busy: false,
        };

        if let Err(e) = self.db.upsert_session(
            &db_id,
            &channel_id_str,
            session.session_id.as_deref(),
            SessionStatus::Idle,
        ) {
            warn!("Failed to upsert session: {e}");
        }

        let arc = Arc::new(tokio::sync::Mutex::new(session));
        self.sessions
            .write()
            .await
            .insert(channel_id_str, arc.clone());
        Ok(arc)
    }

    /// #5: Timeout-wrapped SDK control command with cleanup on timeout.
    pub async fn sdk_control(
        self: &Arc<Self>,
        channel_id_str: &str,
        command: &str,
        params: HashMap<String, serde_json::Value>,
    ) -> Option<()> {
        let session_arc = {
            let sessions = self.sessions.read().await;
            sessions.get(channel_id_str)?.clone()
        };

        let cmd = command.to_string();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.sdk_call_timeout_secs),
            async {
                let mut session = session_arc.lock().await;
                session.process.send_control(&cmd, params).await
            },
        )
        .await;

        match result {
            Ok(Ok(())) => Some(()),
            Ok(Err(e)) => {
                warn!(channel_id = %channel_id_str, error = %e, "SDK control error");
                None
            }
            Err(_) => {
                warn!(channel_id = %channel_id_str, "SDK call timed out, cleaning up");
                self.stop_session(channel_id_str).await;
                None
            }
        }
    }

    pub async fn toggle_mcp_server(
        self: &Arc<Self>,
        channel_id_str: &str,
        server_name: &str,
        enabled: bool,
    ) {
        let mut params = HashMap::new();
        params.insert(
            "server_name".to_string(),
            serde_json::Value::String(server_name.to_string()),
        );
        params.insert("enabled".to_string(), serde_json::Value::Bool(enabled));
        self.sdk_control(channel_id_str, "toggle_mcp_server", params)
            .await;

        // Persist to DB
        let disabled = self.db.get_disabled_mcps(channel_id_str);
        if enabled {
            let updated: Vec<String> = disabled.into_iter().filter(|n| n != server_name).collect();
            if let Err(e) = self.db.set_disabled_mcps(channel_id_str, &updated) {
                warn!("Failed to update disabled MCPs: {e}");
            }
        } else {
            let mut updated = disabled;
            if !updated.contains(&server_name.to_string()) {
                updated.push(server_name.to_string());
            }
            if let Err(e) = self.db.set_disabled_mcps(channel_id_str, &updated) {
                warn!("Failed to update disabled MCPs: {e}");
            }
        }
    }

    async fn handle_tool_use(
        &self,
        channel_id: ChannelId,
        channel_id_str: &str,
        tool_name: &str,
        input: &serde_json::Value,
        request_id: &str,
    ) -> ApprovalDecision {
        if tool_name == "AskUserQuestion" {
            return self
                .handle_ask_user_question(channel_id, channel_id_str, input, request_id)
                .await;
        }

        // #3: Handle file send MCP tool (works even without in-process MCP server,
        // as the tool approval intercepts it and sends the file via Discord)
        if tool_name == "mcp__user__send_file" {
            return self.handle_send_file(channel_id, input).await;
        }

        // Auto-approve read-only tools
        let read_only = ["Read", "Glob", "Grep", "WebSearch", "WebFetch", "TodoWrite"];
        if read_only.contains(&tool_name) {
            return ("allow".into(), None, None);
        }

        // Check auto-approve setting
        if let Some(project) = self.db.get_project(channel_id_str)
            && project.auto_approve
        {
            return ("allow".into(), None, None);
        }

        // Ask user via Discord buttons
        let (embed, row) = create_tool_approval_embed(tool_name, input, request_id);
        self.update_status(channel_id_str, SessionStatus::Waiting);
        let _ = self
            .discord
            .send_message(
                channel_id,
                CreateMessage::new().embed(embed).components(vec![row]),
            )
            .await;

        // Wait for user decision
        let (tx, rx) = oneshot::channel();
        {
            let mut approvals = self.pending_approvals.lock();
            approvals.insert(
                request_id.to_string(),
                PendingApproval {
                    tx,
                    channel_id: channel_id_str.to_string(),
                },
            );
        }

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.approval_timeout_secs),
            rx,
        )
        .await;

        self.update_status(channel_id_str, SessionStatus::Online);

        match result {
            Ok(Ok(decision)) => decision,
            _ => {
                self.pending_approvals.lock().remove(request_id);
                ("deny".into(), Some("Approval timed out".into()), None)
            }
        }
    }

    async fn handle_ask_user_question(
        &self,
        channel_id: ChannelId,
        channel_id_str: &str,
        input: &serde_json::Value,
        _request_id: &str,
    ) -> ApprovalDecision {
        let questions = input
            .get("questions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if questions.is_empty() {
            return ("allow".into(), None, None);
        }

        let mut answers: HashMap<String, serde_json::Value> = HashMap::new();

        for (qi, q_val) in questions.iter().enumerate() {
            let question_data = parse_question_data(q_val);
            let q_request_id = Uuid::new_v4().to_string();

            let (embed, components) =
                create_ask_user_question_embed(&question_data, &q_request_id, qi, questions.len());

            self.update_status(channel_id_str, SessionStatus::Waiting);
            let _ = self
                .discord
                .send_message(
                    channel_id,
                    CreateMessage::new().embed(embed).components(components),
                )
                .await;

            let (tx, rx) = oneshot::channel();
            {
                let mut pending = self.pending_questions.lock();
                pending.insert(
                    q_request_id.clone(),
                    PendingQuestion {
                        tx,
                        channel_id: channel_id_str.to_string(),
                    },
                );
            }

            let answer = tokio::time::timeout(
                std::time::Duration::from_secs(self.approval_timeout_secs),
                rx,
            )
            .await;

            match answer {
                Ok(Ok(Some(ans))) => {
                    answers.insert(question_data.header.clone(), serde_json::Value::String(ans));
                }
                _ => {
                    self.pending_questions.lock().remove(&q_request_id);
                    self.update_status(channel_id_str, SessionStatus::Online);
                    return ("deny".into(), Some("Question timed out".into()), None);
                }
            }
        }

        self.update_status(channel_id_str, SessionStatus::Online);

        let mut updated_input = input.clone();
        if let Some(obj) = updated_input.as_object_mut() {
            obj.insert(
                "answers".to_string(),
                serde_json::to_value(&answers).unwrap(),
            );
        }

        ("allow".into(), None, Some(updated_input))
    }

    async fn handle_send_file(
        &self,
        channel_id: ChannelId,
        input: &serde_json::Value,
    ) -> ApprovalDecision {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ("deny".into(), Some("No file_path provided".into()), None),
        };

        let path = Path::new(file_path);
        if !path.exists() {
            return (
                "deny".into(),
                Some(format!("File not found: {file_path}")),
                None,
            );
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => return ("deny".into(), Some(format!("Cannot read file: {e}")), None),
        };

        if metadata.len() > 25 * 1024 * 1024 {
            return (
                "deny".into(),
                Some("File exceeds Discord's 25MB limit".into()),
                None,
            );
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");

        match tokio::fs::read(path).await {
            Ok(data) => {
                let attachment = CreateAttachment::bytes(data, file_name);
                let _ = self
                    .discord
                    .send_message(channel_id, CreateMessage::new().add_file(attachment))
                    .await;
                ("allow".into(), None, None)
            }
            Err(e) => (
                "deny".into(),
                Some(format!("Failed to send file: {e}")),
                None,
            ),
        }
    }

    // --- Public API for interaction handlers ---

    pub fn resolve_approval(&self, request_id: &str, decision: &str) -> bool {
        let mut approvals = self.pending_approvals.lock();
        if let Some(pending) = approvals.remove(request_id) {
            if decision == "approve-all"
                && let Err(e) = self.db.set_auto_approve(&pending.channel_id, true)
            {
                warn!("Failed to set auto_approve: {e}");
            }
            let behavior = if decision == "deny" { "deny" } else { "allow" };
            let message = if decision == "deny" {
                Some("Denied by user".to_string())
            } else {
                None
            };
            let _ = pending.tx.send((behavior.to_string(), message, None));
            true
        } else {
            false
        }
    }

    pub fn resolve_question(&self, request_id: &str, answer: &str) -> bool {
        let mut questions = self.pending_questions.lock();
        if let Some(pending) = questions.remove(request_id) {
            let _ = pending.tx.send(Some(answer.to_string()));
            true
        } else {
            false
        }
    }

    pub fn enable_custom_input(&self, request_id: &str, channel_id: &str) {
        self.pending_custom_inputs
            .lock()
            .insert(channel_id.to_string(), request_id.to_string());
    }

    pub fn resolve_custom_input(&self, channel_id: &str, text: &str) -> bool {
        let request_id = {
            let mut ci = self.pending_custom_inputs.lock();
            ci.remove(channel_id)
        };
        match request_id {
            Some(rid) => self.resolve_question(&rid, text),
            None => false,
        }
    }

    pub fn has_pending_custom_input(&self, channel_id: &str) -> bool {
        self.pending_custom_inputs.lock().contains_key(channel_id)
    }

    /// Gracefully shut down all active sessions, updating DB statuses to offline.
    pub async fn shutdown(&self) {
        let channels: Vec<String> = self.sessions.read().await.keys().cloned().collect();
        for channel_id in &channels {
            self.stop_session(channel_id).await;
        }
    }

    /// Remove expired entries from pending maps. Call periodically.
    pub fn cleanup_expired_pending(&self) {
        // pending_approvals and pending_questions entries whose oneshot receiver
        // has been dropped (timeout fired) leave dead entries in the map.
        // The sender is still in the map but the receiver is gone — sending will fail.
        // We clean these by attempting to detect closed channels.
        self.pending_approvals
            .lock()
            .retain(|_, pending| !pending.tx.is_closed());
        self.pending_questions
            .lock()
            .retain(|_, pending| !pending.tx.is_closed());
    }

    pub async fn stop_session(&self, channel_id: &str) -> bool {
        let session = self.sessions.write().await.remove(channel_id);
        if let Some(session_arc) = session {
            let mut session = session_arc.lock().await;
            session.process.close().await;
            self.cleanup_pending(channel_id);
            self.update_status(channel_id, SessionStatus::Offline);
            true
        } else {
            false
        }
    }

    pub async fn is_busy(&self, channel_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(channel_id) {
            session.lock().await.busy
        } else {
            false
        }
    }

    pub async fn is_active(&self, channel_id: &str) -> bool {
        self.sessions.read().await.contains_key(channel_id)
    }

    pub fn discord(&self) -> &dyn DiscordClient {
        &*self.discord
    }

    fn cleanup_pending(&self, channel_id: &str) {
        self.pending_approvals
            .lock()
            .retain(|_, v| v.channel_id != channel_id);
        self.pending_questions
            .lock()
            .retain(|_, v| v.channel_id != channel_id);
        self.pending_custom_inputs.lock().remove(channel_id);
    }

    async fn cleanup_session_internal(&self, channel_id: &str) {
        self.sessions.write().await.remove(channel_id);
        self.cleanup_pending(channel_id);
    }

    /// Update session status in DB and broadcast to status channel if configured.
    fn update_status(&self, channel_id: &str, status: SessionStatus) {
        let old = self.db.swap_session_status(channel_id, status);
        if old != status {
            self.notify_status_change(channel_id, old, status);
        }
    }

    /// Send a status change embed to the configured status channel.
    fn notify_status_change(&self, channel_id: &str, old: SessionStatus, new: SessionStatus) {
        let config = get_config();
        let status_ch = match config.status_channel_id {
            Some(id) => ChannelId::new(id),
            None => return,
        };

        let (icon, color) = match new {
            SessionStatus::Online => ("🟢", 0x57f287),
            SessionStatus::Waiting => ("🟡", 0xfee75c),
            SessionStatus::Idle => ("⚪", 0x99aab5),
            SessionStatus::Offline => ("🔴", 0xed4245),
        };

        let ch_id: u64 = channel_id.parse().unwrap_or(0);
        let embed = CreateEmbed::new()
            .title(format!("{icon} Session {}", new.as_str()))
            .description(format!(
                "<#{ch_id}> — **{}** → **{}**",
                old.as_str(),
                new.as_str()
            ))
            .color(color)
            .timestamp(serenity::all::Timestamp::now());

        let discord = self.discord.clone();
        tokio::spawn(async move {
            let _ = discord
                .send_message(status_ch, CreateMessage::new().embed(embed))
                .await;
        });
    }

    // --- Message queue ---

    pub fn set_pending_queue(
        &self,
        channel_id: &str,
        ch_id: ChannelId,
        guild_id: GuildId,
        prompt: &str,
    ) {
        self.pending_queue_prompts.lock().insert(
            channel_id.to_string(),
            QueueItem {
                channel_id: ch_id,
                guild_id,
                prompt: prompt.to_string(),
            },
        );
    }

    pub fn confirm_queue(&self, channel_id: &str) -> bool {
        let pending = self.pending_queue_prompts.lock().remove(channel_id);
        match pending {
            Some(item) => {
                let mut queues = self.message_queues.lock();
                let queue = queues.entry(channel_id.to_string()).or_default();
                queue.push(item);
                true
            }
            None => false,
        }
    }

    pub fn cancel_queue(&self, channel_id: &str) {
        self.pending_queue_prompts.lock().remove(channel_id);
    }

    pub fn is_queue_full(&self, channel_id: &str) -> bool {
        self.message_queues
            .lock()
            .get(channel_id)
            .map(|q| q.len() >= self.max_queue_size)
            .unwrap_or(false)
    }

    pub fn get_queue_size(&self, channel_id: &str) -> usize {
        self.message_queues
            .lock()
            .get(channel_id)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    pub fn has_pending_queue(&self, channel_id: &str) -> bool {
        self.pending_queue_prompts.lock().contains_key(channel_id)
    }

    pub fn get_queue_prompts(&self, channel_id: &str) -> Vec<String> {
        self.message_queues
            .lock()
            .get(channel_id)
            .map(|q| q.iter().map(|i| i.prompt.clone()).collect())
            .unwrap_or_default()
    }

    pub fn clear_queue(&self, channel_id: &str) -> usize {
        let count = self
            .message_queues
            .lock()
            .remove(channel_id)
            .map(|q| q.len())
            .unwrap_or(0);
        self.pending_queue_prompts.lock().remove(channel_id);
        count
    }

    pub fn remove_from_queue(&self, channel_id: &str, index: usize) -> Option<String> {
        let mut queues = self.message_queues.lock();
        let queue = queues.get_mut(channel_id)?;
        if index >= queue.len() {
            return None;
        }
        let removed = queue.remove(index);
        if queue.is_empty() {
            queues.remove(channel_id);
            self.pending_queue_prompts.lock().remove(channel_id);
        }
        Some(removed.prompt)
    }

    /// #1: Actually process the next queued message by calling send_message.
    async fn process_queue(self: &Arc<Self>, channel_id: &str) {
        let next = {
            let mut queues = self.message_queues.lock();
            let queue = match queues.get_mut(channel_id) {
                Some(q) if !q.is_empty() => q,
                _ => return,
            };
            let item = queue.remove(0);
            if queue.is_empty() {
                queues.remove(channel_id);
            }
            item
        };

        let remaining = self.get_queue_size(channel_id);
        let preview = if next.prompt.len() > 40 {
            format!("{}…", truncate(&next.prompt, 40))
        } else {
            next.prompt.clone()
        };
        let msg = if remaining > 0 {
            format!("📨 Processing queued message... (remaining: {remaining})\n> {preview}")
        } else {
            format!("📨 Processing queued message...\n> {preview}")
        };
        let _ = self
            .discord
            .send_message(next.channel_id, CreateMessage::new().content(msg))
            .await;

        let self_clone = self.clone();
        let prompt = next.prompt;
        let ch_id = next.channel_id;
        let g_id = next.guild_id;

        tokio::spawn(async move {
            if let Err(e) = self_clone.send_message(ch_id, g_id, &prompt).await {
                error!(channel_id = %ch_id, error = %e, "queued message failed");
                let _ = self_clone
                    .discord()
                    .send_message(
                        ch_id,
                        CreateMessage::new().content(format!("❌ Queued message failed: {e}")),
                    )
                    .await;
            }
        });
    }
}

fn parse_question_data(val: &serde_json::Value) -> AskQuestionData {
    let question = val
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            warn!(data = %val, "question field missing or not a string in SDK question data");
            ""
        })
        .to_string();
    let header = val
        .get("header")
        .and_then(|v| v.as_str())
        .unwrap_or("Question")
        .to_string();
    let multi_select = val
        .get("multiSelect")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let options = match val.get("options").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .map(|o| AskOption {
                label: o
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: o
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
            .collect(),
        None => {
            warn!(data = %val, "options field missing or not an array in SDK question data");
            vec![]
        }
    };

    AskQuestionData {
        question,
        header,
        options,
        multi_select,
    }
}
