use crate::config::Config;
use crate::events::{Event, EventHandler};
use crate::generated::temporal::api::{
    history::v1::HistoryEvent, workflow::v1::WorkflowExecutionInfo,
    workflowservice::v1::DescribeNamespaceResponse,
};
use crate::temporal::{
    event_format::structured_attributes,
    tree::{build_outline, OutlineRow, WorkflowSnapshot},
    TemporalClient,
};
use crate::ui;
use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::{backend::Backend, widgets::TableState, Terminal};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Workflows,
    Namespaces,
    WorkflowDetail,
    Help,
}

/// Commands that can be sent to the async task handler
#[derive(Debug, Clone)]
pub enum AppCommand {
    LoadWorkflows(String, Vec<u8>, u64), // query, page token, request ID
    ViewWorkflowDetail(WorkflowExecutionInfo, u64), // selected execution, request ID
    RefreshNamespaces,
    SwitchNamespace(String),
    TerminateWorkflow(String, String, String), // workflow_id, run_id, reason
    CancelWorkflow(String, String),            // workflow_id, run_id
    SignalWorkflow(String, String, String),    // workflow_id, run_id, signal_name
}

/// Results from async operations
#[derive(Debug, Clone)]
pub enum AppResult {
    WorkflowsLoaded {
        workflows: Vec<WorkflowExecutionInfo>,
        next_page_token: Vec<u8>,
        request_id: u64,
    },
    WorkflowsError(u64, String),
    WorkflowDetailLoaded {
        workflow: WorkflowExecutionInfo,
        history: Vec<HistoryEvent>,
        outline: Vec<OutlineRow>,
        outline_note: Option<String>,
        request_id: u64,
    },
    WorkflowDetailError(u64, String),
    NamespacesLoaded {
        namespaces: Vec<DescribeNamespaceResponse>,
    },
    NamespacesError(String),
    NamespaceSwitched {
        namespace: String,
    },
    WorkflowOperationSuccess(String), // operation description
    WorkflowOperationError(String),   // error message
}

/// State for the workflow list screen
#[derive(Debug, Clone)]
pub struct WorkflowListState {
    pub items: Vec<WorkflowExecutionInfo>,
    pub table_state: TableState,
    pub next_page_token: Vec<u8>,
    pub prev_page_tokens: Vec<Vec<u8>>, // Tokens used to fetch preceding pages
    pub current_page_token: Vec<u8>,
    pub request_id: u64,
    pub loading: bool,
    pub error: Option<String>,
    pub current_page: usize,
    pub query: String,
    pub query_history: Vec<String>,
    pub input_mode: bool,
    pub active_filter: Option<WorkflowFilter>,
    pub show_child_workflows: bool,
    pub auto_refresh_enabled: bool,
    pub auto_refresh_interval_secs: u64,
    pub last_refresh: Option<std::time::Instant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowFilter {
    All,
    Running,
    Completed,
    Failed,
    Canceled,
}

impl WorkflowListState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            table_state: TableState::default(),
            next_page_token: Vec::new(),
            prev_page_tokens: Vec::new(),
            current_page_token: Vec::new(),
            request_id: 0,
            loading: false,
            error: None,
            current_page: 1,
            query: String::new(),
            query_history: Vec::new(),
            input_mode: false,
            active_filter: None,
            show_child_workflows: false,
            auto_refresh_enabled: false,
            auto_refresh_interval_secs: 5, // Default 5 seconds
            last_refresh: None,
        }
    }

    pub fn should_refresh(&self) -> bool {
        if !self.auto_refresh_enabled || self.loading {
            return false;
        }

        match self.last_refresh {
            Some(last) => {
                let elapsed = last.elapsed().as_secs();
                elapsed >= self.auto_refresh_interval_secs
            }
            None => true, // Never refreshed, should refresh
        }
    }

    pub fn mark_refreshed(&mut self) {
        self.last_refresh = Some(std::time::Instant::now());
    }

    pub fn get_query(&self) -> String {
        // Build query from active filter and custom query
        let mut queries = Vec::new();
        if !self.show_child_workflows {
            queries.push("ParentWorkflowId IS NULL".to_string());
        }

        if let Some(filter) = &self.active_filter {
            let filter_query = match filter {
                WorkflowFilter::All => None,
                WorkflowFilter::Running => Some("ExecutionStatus = 'Running'"),
                WorkflowFilter::Completed => Some("ExecutionStatus = 'Completed'"),
                WorkflowFilter::Failed => Some("ExecutionStatus = 'Failed'"),
                WorkflowFilter::Canceled => Some("ExecutionStatus = 'Canceled'"),
            };
            if let Some(fq) = filter_query {
                queries.push(fq.to_string());
            }
        }

        if !self.query.is_empty() {
            // Preserve the parent constraint even when a custom query contains OR.
            queries.push(format!("({})", self.query));
        }

        queries.join(" AND ")
    }

    pub fn has_next_page(&self) -> bool {
        !self.next_page_token.is_empty()
    }

    pub fn has_prev_page(&self) -> bool {
        !self.prev_page_tokens.is_empty()
    }

    // Page tokens identify the page to request, not the page returned by the server.
    pub fn begin_next_page(&mut self) -> Option<Vec<u8>> {
        if self.loading || !self.has_next_page() {
            return None;
        }
        self.prev_page_tokens.push(self.current_page_token.clone());
        self.current_page_token = self.next_page_token.clone();
        self.current_page += 1;
        self.loading = true;
        Some(self.current_page_token.clone())
    }

    pub fn begin_previous_page(&mut self) -> Option<Vec<u8>> {
        if self.loading {
            return None;
        }
        let token = self.prev_page_tokens.pop()?;
        self.current_page_token = token.clone();
        self.current_page = self.current_page.saturating_sub(1).max(1);
        self.loading = true;
        Some(token)
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn selected_workflow(&self) -> Option<&WorkflowExecutionInfo> {
        self.table_state.selected().and_then(|i| self.items.get(i))
    }
}

/// State for the workflow detail screen
#[derive(Debug, Clone)]
pub struct WorkflowDetailState {
    pub workflow: Option<WorkflowExecutionInfo>,
    pub history: Vec<HistoryEvent>,
    pub outline: Vec<OutlineRow>,
    pub outline_note: Option<String>,
    pub show_history: bool,
    pub outline_state: TableState,
    pub auto_refresh_enabled: bool,
    pub last_refresh: Option<std::time::Instant>,
    pub table_state: TableState,
    pub loading: bool,
    pub refreshing: bool,
    pub error: Option<String>,
    pub show_dialog: Option<WorkflowOperation>,
    pub dialog_workflow: Option<WorkflowExecutionInfo>,
    pub dialog_input: String,
    pub success_message: Option<String>,
    pub show_event_detail: bool,
    pub event_detail_lines: Vec<String>,
    pub event_detail_scroll_offset: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowOperation {
    Terminate,
    Cancel,
    Signal,
}

impl WorkflowDetailState {
    pub fn new() -> Self {
        Self {
            workflow: None,
            history: Vec::new(),
            outline: Vec::new(),
            outline_note: None,
            show_history: false,
            outline_state: TableState::default(),
            auto_refresh_enabled: true,
            last_refresh: None,
            table_state: TableState::default(),
            loading: false,
            refreshing: false,
            error: None,
            show_dialog: None,
            dialog_workflow: None,
            dialog_input: String::new(),
            success_message: None,
            show_event_detail: false,
            event_detail_lines: Vec::new(),
            event_detail_scroll_offset: 0,
        }
    }

    pub fn selected_event(&self) -> Option<&HistoryEvent> {
        self.table_state
            .selected()
            .and_then(|i| self.history.get(i))
    }

    pub fn select_next(&mut self) {
        if self.show_history {
            Self::advance(&mut self.table_state, self.history.len(), true);
        } else {
            Self::advance(&mut self.outline_state, self.outline.len(), true);
        }
    }

    pub fn select_previous(&mut self) {
        if self.show_history {
            Self::advance(&mut self.table_state, self.history.len(), false);
        } else {
            Self::advance(&mut self.outline_state, self.outline.len(), false);
        }
    }

    fn advance(state: &mut TableState, len: usize, forward: bool) {
        if len == 0 {
            return;
        }
        let current = state.selected().unwrap_or(0);
        state.select(Some(if forward {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        }));
    }

    pub fn selected_outline_workflow(&self) -> Option<&WorkflowExecutionInfo> {
        self.outline_state
            .selected()
            .and_then(|i| self.outline.get(i))
            .and_then(|r| r.workflow.as_ref())
    }
}

/// State for the namespace list screen
#[derive(Debug, Clone)]
pub struct NamespaceListState {
    pub items: Vec<DescribeNamespaceResponse>,
    pub table_state: TableState,
    pub loading: bool,
    pub error: Option<String>,
}

impl NamespaceListState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            table_state: TableState::default(),
            loading: false,
            error: None,
        }
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn selected_namespace(&self) -> Option<&DescribeNamespaceResponse> {
        self.table_state.selected().and_then(|i| self.items.get(i))
    }
}

/// State for the help screen
#[derive(Debug, Clone)]
pub struct HelpState {
    pub scroll_offset: u16,
}

impl HelpState {
    pub fn new() -> Self {
        Self { scroll_offset: 0 }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }
}

pub struct App {
    pub config: Config,
    pub running: bool,
    pub current_screen: Screen,
    pub event_handler: EventHandler,
    pub client: Option<TemporalClient>,
    pub workflow_list_state: WorkflowListState,
    pub workflow_detail_state: WorkflowDetailState,
    pub namespace_list_state: NamespaceListState,
    pub help_state: HelpState,
    pub connection_status: ConnectionStatus,
    pub current_namespace: String,
    pub frame_count: u16,
    detail_request_id: u64,
    detail_back_stack: Vec<WorkflowExecutionInfo>,
    command_tx: mpsc::UnboundedSender<AppCommand>,
    result_rx: mpsc::UnboundedReceiver<AppResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl App {
    pub async fn new(target: Option<crate::cli::WorkflowTarget>) -> Result<Self> {
        let config = Config::load()?;
        let event_handler = EventHandler::new();

        // Create channels for async communication
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (result_tx, result_rx) = mpsc::unbounded_channel();

        // Get initial namespace from config
        let initial_namespace = config
            .get_active_profile()
            .map(|p| p.namespace.clone())
            .unwrap_or_else(|| "default".to_string());

        let mut app = Self {
            config,
            running: true,
            current_screen: Screen::Workflows,
            event_handler,
            client: None,
            workflow_list_state: WorkflowListState::new(),
            workflow_detail_state: WorkflowDetailState::new(),
            namespace_list_state: NamespaceListState::new(),
            help_state: HelpState::new(),
            connection_status: ConnectionStatus::Disconnected,
            current_namespace: initial_namespace,
            frame_count: 0,
            detail_request_id: 0,
            detail_back_stack: Vec::new(),
            command_tx,
            result_rx,
        };

        // Connect to Temporal
        app.connect_temporal().await?;

        // A direct link describes the exact execution before the TUI starts. An empty
        // run ID asks Temporal for the latest run of this workflow ID.
        let initial_detail = if let Some(target) = target {
            let client = app.client.as_mut().ok_or_else(|| {
                anyhow::anyhow!(
                    "{}",
                    match &app.connection_status {
                        ConnectionStatus::Error(message) => message.as_str(),
                        _ => "Temporal connection unavailable",
                    }
                )
            })?;
            let response = client
                .describe_workflow_execution(
                    &target.workflow_id,
                    target.run_id.as_deref().unwrap_or(""),
                )
                .await?;
            Some(response.workflow_execution_info.ok_or_else(|| {
                anyhow::anyhow!(
                    "Temporal did not return execution details for {}",
                    target.workflow_id
                )
            })?)
        } else {
            None
        };

        // Spawn async task handler only after a successful connection.
        let client = app.client.take().ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                match &app.connection_status {
                    ConnectionStatus::Error(message) => message.as_str(),
                    _ => "Temporal connection unavailable",
                }
            )
        })?;
        app.spawn_task_handler(client, command_rx, result_tx);

        if let Some(workflow) = initial_detail {
            app.current_screen = Screen::WorkflowDetail;
            app.load_detail(workflow, true);
        } else {
            // The standard launch still opens the parent-only workflow list.
            app.workflow_list_state.loading = true;
            app.load_workflows(app.workflow_list_state.get_query(), Vec::new())?;
        }

        Ok(app)
    }

    fn spawn_task_handler(
        &self,
        mut client: TemporalClient,
        mut command_rx: mpsc::UnboundedReceiver<AppCommand>,
        result_tx: mpsc::UnboundedSender<AppResult>,
    ) {
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                match command {
                    AppCommand::LoadWorkflows(query, page_token, request_id) => {
                        tracing::info!("Loading workflows with query: '{}'", query);
                        match client.list_workflow_executions(50, page_token, query).await {
                            Ok(response) => {
                                let _ = result_tx.send(AppResult::WorkflowsLoaded {
                                    workflows: response.executions,
                                    next_page_token: response.next_page_token,
                                    request_id,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowsError(
                                    request_id,
                                    format!("Failed to load workflows: {}", e),
                                ));
                            }
                        }
                    }
                    AppCommand::ViewWorkflowDetail(workflow, request_id) => {
                        if let Some(execution) = &workflow.execution {
                            let execution = execution.clone();
                            match client
                                .history_events(&execution.workflow_id, &execution.run_id)
                                .await
                            {
                                Ok(history) => {
                                    let root_id = workflow
                                        .root_execution
                                        .as_ref()
                                        .unwrap_or(&execution)
                                        .workflow_id
                                        .clone();
                                    let query = format!(
                                        "RootWorkflowId = '{}'",
                                        root_id.replace('\'', "''")
                                    );
                                    let mut infos = Vec::new();
                                    let mut page_token = Vec::new();
                                    let mut outline_note = None;
                                    loop {
                                        match client
                                            .list_workflow_executions(
                                                100,
                                                page_token.clone(),
                                                query.clone(),
                                            )
                                            .await
                                        {
                                            Ok(response) => {
                                                infos.extend(response.executions);
                                                if response.next_page_token.is_empty() {
                                                    break;
                                                }
                                                if response.next_page_token == page_token
                                                    || infos.len() >= 100
                                                {
                                                    outline_note = Some(
                                                        "Outline limited to 100 workflows".into(),
                                                    );
                                                    break;
                                                }
                                                page_token = response.next_page_token;
                                            }
                                            Err(e) => {
                                                outline_note = Some(format!(
                                                    "Child workflow lookup failed: {}",
                                                    e
                                                ));
                                                break;
                                            }
                                        }
                                    }
                                    let selected_from_list = infos
                                        .iter()
                                        .find(|info| info.execution.as_ref() == Some(&execution))
                                        .cloned()
                                        .unwrap_or(workflow);
                                    let (selected, pending_activities, pending_children) =
                                        match client
                                            .describe_workflow_execution(
                                                &execution.workflow_id,
                                                &execution.run_id,
                                            )
                                            .await
                                        {
                                            Ok(description) => (
                                                description
                                                    .workflow_execution_info
                                                    .unwrap_or(selected_from_list),
                                                description.pending_activities,
                                                description.pending_children,
                                            ),
                                            Err(e) => {
                                                outline_note = Some(format!(
                                                    "Live activity status unavailable: {}",
                                                    e
                                                ));
                                                (selected_from_list, Vec::new(), Vec::new())
                                            }
                                        };
                                    let mut snapshots = vec![WorkflowSnapshot {
                                        info: selected.clone(),
                                        history: history.clone(),
                                        pending_activities,
                                        pending_children,
                                    }];
                                    for info in infos {
                                        let Some(child) = &info.execution else {
                                            continue;
                                        };
                                        if child == &execution {
                                            continue;
                                        }
                                        let child_id = child.workflow_id.clone();
                                        let child_run = child.run_id.clone();
                                        let child_history = match client
                                            .history_events(&child_id, &child_run)
                                            .await
                                        {
                                            Ok(events) => events,
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Could not read child history {}: {}",
                                                    child_id,
                                                    e
                                                );
                                                outline_note = Some(
                                                    "Some child histories could not be loaded"
                                                        .into(),
                                                );
                                                Vec::new()
                                            }
                                        };
                                        let (info, pending_activities, pending_children) =
                                            match client
                                                .describe_workflow_execution(&child_id, &child_run)
                                                .await
                                            {
                                                Ok(description) => (
                                                    description
                                                        .workflow_execution_info
                                                        .unwrap_or(info),
                                                    description.pending_activities,
                                                    description.pending_children,
                                                ),
                                                Err(e) => {
                                                    tracing::warn!(
                                                        "Could not describe child {}: {}",
                                                        child_id,
                                                        e
                                                    );
                                                    outline_note = Some(
                                                        "Some live child statuses unavailable"
                                                            .into(),
                                                    );
                                                    (info, Vec::new(), Vec::new())
                                                }
                                            };
                                        snapshots.push(WorkflowSnapshot {
                                            info,
                                            history: child_history,
                                            pending_activities,
                                            pending_children,
                                        });
                                    }
                                    let outline = build_outline(&snapshots, &selected);
                                    let _ = result_tx.send(AppResult::WorkflowDetailLoaded {
                                        workflow: selected,
                                        history,
                                        outline,
                                        outline_note,
                                        request_id,
                                    });
                                }
                                Err(e) => {
                                    let _ = result_tx.send(AppResult::WorkflowDetailError(
                                        request_id,
                                        format!("Failed to load workflow detail: {}", e),
                                    ));
                                }
                            }
                        }
                    }
                    AppCommand::RefreshNamespaces => {
                        tracing::info!("Loading namespaces");
                        match client.list_namespaces(50, Vec::new()).await {
                            Ok(response) => {
                                let _ = result_tx.send(AppResult::NamespacesLoaded {
                                    namespaces: response.namespaces,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::NamespacesError(format!(
                                    "Failed to load namespaces: {}",
                                    e
                                )));
                            }
                        }
                    }
                    AppCommand::SwitchNamespace(namespace) => {
                        tracing::info!("Switching to namespace: {}", namespace);
                        client.set_namespace(namespace.clone());
                        let _ = result_tx.send(AppResult::NamespaceSwitched { namespace });
                    }
                    AppCommand::TerminateWorkflow(workflow_id, run_id, reason) => {
                        tracing::info!(
                            "Terminating workflow: {} with reason: {}",
                            workflow_id,
                            reason
                        );
                        match client
                            .terminate_workflow(workflow_id.clone(), run_id, reason)
                            .await
                        {
                            Ok(_) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationSuccess(
                                    format!("Workflow {} terminated successfully", workflow_id),
                                ));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(format!(
                                    "Failed to terminate workflow: {}",
                                    e
                                )));
                            }
                        }
                    }
                    AppCommand::CancelWorkflow(workflow_id, run_id) => {
                        tracing::info!("Canceling workflow: {}", workflow_id);
                        match client.cancel_workflow(workflow_id.clone(), run_id).await {
                            Ok(_) => {
                                let _ =
                                    result_tx.send(AppResult::WorkflowOperationSuccess(format!(
                                        "Workflow {} cancel requested successfully",
                                        workflow_id
                                    )));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(format!(
                                    "Failed to cancel workflow: {}",
                                    e
                                )));
                            }
                        }
                    }
                    AppCommand::SignalWorkflow(workflow_id, run_id, signal_name) => {
                        tracing::info!(
                            "Signaling workflow: {} with signal: {}",
                            workflow_id,
                            signal_name
                        );
                        match client
                            .signal_workflow(workflow_id.clone(), run_id, signal_name.clone())
                            .await
                        {
                            Ok(_) => {
                                let _ =
                                    result_tx.send(AppResult::WorkflowOperationSuccess(format!(
                                        "Signal '{}' sent to workflow {} successfully",
                                        signal_name, workflow_id
                                    )));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(format!(
                                    "Failed to signal workflow: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
            }
        });
    }

    async fn connect_temporal(&mut self) -> Result<()> {
        self.connection_status = ConnectionStatus::Connecting;

        let profile = self.config.get_active_profile();
        if let Some(profile) = profile {
            match TemporalClient::from_profile(profile).await {
                Ok(client) => {
                    self.connection_status = ConnectionStatus::Connected;
                    self.client = Some(client);
                    tracing::info!("Successfully connected to Temporal");
                }
                Err(e) => {
                    let error_msg = format!("Connection failed: {}", e);
                    self.connection_status = ConnectionStatus::Error(error_msg.clone());
                    tracing::error!("{}", error_msg);
                }
            }
        } else {
            let error_msg = "No active profile configured".to_string();
            self.connection_status = ConnectionStatus::Error(error_msg.clone());
            tracing::error!("{}", error_msg);
        }

        Ok(())
    }

    fn load_workflows(&mut self, query: String, page_token: Vec<u8>) -> Result<()> {
        self.workflow_list_state.request_id = self.workflow_list_state.request_id.wrapping_add(1);
        self.command_tx.send(AppCommand::LoadWorkflows(
            query,
            page_token,
            self.workflow_list_state.request_id,
        ))?;
        Ok(())
    }

    fn load_detail(&mut self, workflow: WorkflowExecutionInfo, reset: bool) {
        if reset {
            self.workflow_detail_state = WorkflowDetailState::new();
        }
        self.detail_request_id = self.detail_request_id.wrapping_add(1);
        if reset {
            self.workflow_detail_state.loading = true;
        } else {
            self.workflow_detail_state.refreshing = true;
        }
        let _ = self.command_tx.send(AppCommand::ViewWorkflowDetail(
            workflow,
            self.detail_request_id,
        ));
    }

    fn process_results(&mut self) {
        // Process all available results from async tasks
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                AppResult::WorkflowsLoaded {
                    workflows,
                    next_page_token,
                    request_id,
                } => {
                    if request_id != self.workflow_list_state.request_id {
                        continue;
                    }
                    self.workflow_list_state.items = workflows;
                    self.workflow_list_state.next_page_token = next_page_token;
                    self.workflow_list_state.loading = false;
                    self.workflow_list_state.error = None;
                    self.workflow_list_state.mark_refreshed();

                    // Select first item if list is not empty
                    if !self.workflow_list_state.items.is_empty() {
                        self.workflow_list_state.table_state.select(Some(0));
                    }

                    tracing::info!(
                        "Loaded {} workflows (page {})",
                        self.workflow_list_state.items.len(),
                        self.workflow_list_state.current_page
                    );
                }
                AppResult::WorkflowsError(request_id, error) => {
                    if request_id != self.workflow_list_state.request_id {
                        continue;
                    }
                    self.workflow_list_state.current_page = 1;
                    self.workflow_list_state.current_page_token.clear();
                    self.workflow_list_state.prev_page_tokens.clear();
                    self.workflow_list_state.items.clear();
                    self.workflow_list_state.next_page_token.clear();
                    self.workflow_list_state.error = Some(error.clone());
                    self.workflow_list_state.loading = false;
                    tracing::error!("{}", error);
                }
                AppResult::WorkflowDetailLoaded {
                    workflow,
                    history,
                    outline,
                    outline_note,
                    request_id,
                } => {
                    if request_id != self.detail_request_id {
                        continue;
                    }
                    self.workflow_detail_state.workflow = Some(workflow);
                    self.workflow_detail_state.history = history;
                    self.workflow_detail_state.outline = outline;
                    self.workflow_detail_state.outline_note = outline_note;
                    if let Some(index) = self.workflow_detail_state.outline_state.selected() {
                        let len = self.workflow_detail_state.outline.len();
                        self.workflow_detail_state
                            .outline_state
                            .select((len > 0).then_some(index.min(len.saturating_sub(1))));
                    }
                    self.workflow_detail_state.last_refresh = Some(std::time::Instant::now());
                    if !self.workflow_detail_state.outline.is_empty()
                        && self
                            .workflow_detail_state
                            .outline_state
                            .selected()
                            .is_none()
                    {
                        self.workflow_detail_state.outline_state.select(Some(0));
                    }
                    self.workflow_detail_state.loading = false;
                    self.workflow_detail_state.refreshing = false;
                    self.workflow_detail_state.error = None;

                    // Select first event if list is not empty
                    if !self.workflow_detail_state.history.is_empty()
                        && self.workflow_detail_state.table_state.selected().is_none()
                    {
                        self.workflow_detail_state.table_state.select(Some(0));
                    }

                    tracing::info!(
                        "Loaded {} history events",
                        self.workflow_detail_state.history.len()
                    );
                }
                AppResult::WorkflowDetailError(request_id, error) => {
                    if request_id != self.detail_request_id {
                        continue;
                    }
                    if self.workflow_detail_state.workflow.is_some() {
                        self.workflow_detail_state.outline_note = Some(error.clone());
                    } else {
                        self.workflow_detail_state.error = Some(error.clone());
                    }
                    self.workflow_detail_state.loading = false;
                    self.workflow_detail_state.refreshing = false;
                    self.workflow_detail_state.last_refresh = Some(std::time::Instant::now());
                    tracing::error!("{}", error);
                }
                AppResult::NamespacesLoaded { namespaces } => {
                    self.namespace_list_state.items = namespaces;
                    self.namespace_list_state.loading = false;
                    self.namespace_list_state.error = None;

                    // Select first item if list is not empty
                    if !self.namespace_list_state.items.is_empty()
                        && self.namespace_list_state.table_state.selected().is_none()
                    {
                        self.namespace_list_state.table_state.select(Some(0));
                    }

                    tracing::info!(
                        "Loaded {} namespaces",
                        self.namespace_list_state.items.len()
                    );
                }
                AppResult::NamespacesError(error) => {
                    self.namespace_list_state.error = Some(error.clone());
                    self.namespace_list_state.loading = false;
                    tracing::error!("{}", error);
                }
                AppResult::NamespaceSwitched { namespace } => {
                    self.current_namespace = namespace.clone();
                    tracing::info!("Switched to namespace: {}", namespace);
                    // Refresh workflows after switching namespace
                    self.workflow_list_state.prev_page_tokens.clear();
                    self.workflow_list_state.current_page_token.clear();
                    self.workflow_list_state.current_page = 1;
                    self.workflow_list_state.items.clear();
                    self.workflow_list_state.loading = true;
                    let query = self.workflow_list_state.get_query();
                    let _ = self.load_workflows(query, Vec::new());
                    // Switch back to workflows screen
                    self.current_screen = Screen::Workflows;
                }
                AppResult::WorkflowOperationSuccess(message) => {
                    self.workflow_detail_state.success_message = Some(message.clone());
                    self.workflow_detail_state.show_dialog = None;
                    self.workflow_detail_state.dialog_input.clear();
                    tracing::info!("{}", message);
                }
                AppResult::WorkflowOperationError(error) => {
                    self.workflow_detail_state.error = Some(error.clone());
                    self.workflow_detail_state.show_dialog = None;
                    self.workflow_detail_state.dialog_input.clear();
                    tracing::error!("{}", error);
                }
            }
        }
    }

    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        <B as Backend>::Error: Send + Sync + 'static,
    {
        while self.running {
            // Process any async results
            self.process_results();

            // Check if auto-refresh is needed (only on Workflows screen)
            if matches!(self.current_screen, Screen::Workflows)
                && self.workflow_list_state.should_refresh()
            {
                tracing::debug!("Auto-refreshing workflows");
                self.workflow_list_state.prev_page_tokens.clear();
                self.workflow_list_state.current_page_token.clear();
                self.workflow_list_state.current_page = 1;
                self.workflow_list_state.loading = true;
                let query = self.workflow_list_state.get_query();
                let _ = self.load_workflows(query, Vec::new());
            }

            if matches!(self.current_screen, Screen::WorkflowDetail) {
                let state = &self.workflow_detail_state;
                if state.auto_refresh_enabled
                    && !state.loading
                    && state.show_dialog.is_none()
                    && !state.show_event_detail
                    && state.success_message.is_none()
                    && state
                        .last_refresh
                        .is_some_and(|last| last.elapsed().as_secs() >= 5)
                {
                    if let Some(workflow) = state.workflow.clone() {
                        self.load_detail(workflow, false);
                    }
                }
            }

            terminal.draw(|f| ui::render(&self, f))?;

            // Increment frame count for animations
            self.frame_count = self.frame_count.wrapping_add(1);

            if let Event::Key(key) = self.event_handler.next()? {
                self.handle_key(key.code)?;
            }
        }

        Ok(())
    }

    pub fn spinner(&self) -> &str {
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let index = (self.frame_count / 3) as usize % frames.len();
        frames[index]
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        match self.current_screen {
            Screen::Workflows => {
                // Handle input mode separately
                if self.workflow_list_state.input_mode {
                    match key {
                        KeyCode::Char(c) => {
                            self.workflow_list_state.query.push(c);
                        }
                        KeyCode::Backspace => {
                            self.workflow_list_state.query.pop();
                        }
                        KeyCode::Enter => {
                            // Save to history if non-empty
                            if !self.workflow_list_state.query.is_empty() {
                                self.workflow_list_state
                                    .query_history
                                    .push(self.workflow_list_state.query.clone());
                            }
                            // Exit input mode and refresh (reset to page 1)
                            self.workflow_list_state.input_mode = false;
                            self.workflow_list_state.loading = true;
                            self.workflow_list_state.prev_page_tokens.clear();
                            self.workflow_list_state.current_page_token.clear();
                            self.workflow_list_state.current_page = 1;
                            let query = self.workflow_list_state.get_query();
                            let _ = self.load_workflows(query, Vec::new());
                        }
                        KeyCode::Esc => {
                            // Exit input mode without searching
                            self.workflow_list_state.input_mode = false;
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                // Normal mode key handling
                match key {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        self.running = false;
                    }
                    KeyCode::Char('1') => {
                        self.current_screen = Screen::Workflows;
                    }
                    KeyCode::Char('2') => {
                        self.current_screen = Screen::Namespaces;
                        // Load namespaces if empty
                        if self.namespace_list_state.items.is_empty()
                            && !self.namespace_list_state.loading
                        {
                            self.namespace_list_state.loading = true;
                            let _ = self.command_tx.send(AppCommand::RefreshNamespaces);
                        }
                    }
                    KeyCode::Char('?') => {
                        self.help_state.reset_scroll();
                        self.current_screen = Screen::Help;
                    }
                    KeyCode::Char('/') => {
                        // Enter search mode
                        self.workflow_list_state.input_mode = true;
                        self.workflow_list_state.query.clear();
                    }
                    KeyCode::Char('f') => {
                        // Cycle through filters
                        self.workflow_list_state.active_filter =
                            match self.workflow_list_state.active_filter {
                                None => Some(WorkflowFilter::Running),
                                Some(WorkflowFilter::Running) => Some(WorkflowFilter::Completed),
                                Some(WorkflowFilter::Completed) => Some(WorkflowFilter::Failed),
                                Some(WorkflowFilter::Failed) => Some(WorkflowFilter::Canceled),
                                Some(WorkflowFilter::Canceled) => Some(WorkflowFilter::All),
                                Some(WorkflowFilter::All) => None,
                            };
                        // Refresh with new filter (reset to page 1)
                        self.workflow_list_state.loading = true;
                        self.workflow_list_state.prev_page_tokens.clear();
                        self.workflow_list_state.current_page_token.clear();
                        self.workflow_list_state.current_page = 1;
                        let query = self.workflow_list_state.get_query();
                        let _ = self.load_workflows(query, Vec::new());
                    }
                    KeyCode::Char('c') => {
                        // Clear filter and search (reset to page 1)
                        self.workflow_list_state.active_filter = None;
                        self.workflow_list_state.query.clear();
                        self.workflow_list_state.loading = true;
                        self.workflow_list_state.prev_page_tokens.clear();
                        self.workflow_list_state.current_page_token.clear();
                        self.workflow_list_state.current_page = 1;
                        let query = self.workflow_list_state.get_query();
                        let _ = self.load_workflows(query, Vec::new());
                    }
                    KeyCode::Char('v') => {
                        // Visibility scope is part of the server query so pages remain full.
                        self.workflow_list_state.show_child_workflows =
                            !self.workflow_list_state.show_child_workflows;
                        self.workflow_list_state.loading = true;
                        self.workflow_list_state.prev_page_tokens.clear();
                        self.workflow_list_state.current_page_token.clear();
                        self.workflow_list_state.current_page = 1;
                        self.workflow_list_state.next_page_token.clear();
                        self.workflow_list_state.items.clear();
                        self.workflow_list_state.table_state.select(None);
                        let query = self.workflow_list_state.get_query();
                        let _ = self.load_workflows(query, Vec::new());
                    }
                    KeyCode::Char('a') => {
                        // Toggle auto-refresh
                        self.workflow_list_state.auto_refresh_enabled =
                            !self.workflow_list_state.auto_refresh_enabled;
                        if self.workflow_list_state.auto_refresh_enabled {
                            tracing::info!(
                                "Auto-refresh enabled ({}s interval)",
                                self.workflow_list_state.auto_refresh_interval_secs
                            );
                        } else {
                            tracing::info!("Auto-refresh disabled");
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.workflow_list_state.select_next();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.workflow_list_state.select_previous();
                    }
                    KeyCode::Char('r') => {
                        // Refresh workflows with current query (reset to page 1)
                        self.workflow_list_state.loading = true;
                        self.workflow_list_state.prev_page_tokens.clear();
                        self.workflow_list_state.current_page_token.clear();
                        self.workflow_list_state.current_page = 1;
                        let query = self.workflow_list_state.get_query();
                        let _ = self.load_workflows(query, Vec::new());
                    }
                    KeyCode::Char('n') | KeyCode::Right => {
                        if let Some(token) = self.workflow_list_state.begin_next_page() {
                            let query = self.workflow_list_state.get_query();
                            let _ = self.load_workflows(query, token);
                        }
                    }
                    KeyCode::Char('p') | KeyCode::Left => {
                        if let Some(token) = self.workflow_list_state.begin_previous_page() {
                            let query = self.workflow_list_state.get_query();
                            let _ = self.load_workflows(query, token);
                        }
                    }
                    KeyCode::Enter => {
                        // View workflow detail
                        if let Some(workflow) = self.workflow_list_state.selected_workflow() {
                            if let Some(execution) = &workflow.execution {
                                tracing::info!("Viewing workflow: {}", execution.workflow_id);
                                let workflow = workflow.clone();
                                self.detail_back_stack.clear();
                                self.load_detail(workflow, true);
                                self.current_screen = Screen::WorkflowDetail;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Screen::Namespaces => match key {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.current_screen = Screen::Workflows;
                }
                KeyCode::Char('1') => {
                    self.current_screen = Screen::Workflows;
                }
                KeyCode::Char('2') => {
                    self.current_screen = Screen::Namespaces;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.namespace_list_state.select_next();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.namespace_list_state.select_previous();
                }
                KeyCode::Char('r') => {
                    // Refresh namespaces
                    self.namespace_list_state.loading = true;
                    let _ = self.command_tx.send(AppCommand::RefreshNamespaces);
                }
                KeyCode::Enter => {
                    // Switch to selected namespace
                    if let Some(ns_response) = self.namespace_list_state.selected_namespace() {
                        if let Some(ns_info) = &ns_response.namespace_info {
                            let namespace_name = ns_info.name.clone();
                            tracing::info!("Switching to namespace: {}", namespace_name);
                            let _ = self
                                .command_tx
                                .send(AppCommand::SwitchNamespace(namespace_name));
                        }
                    }
                }
                _ => {}
            },
            Screen::WorkflowDetail => {
                if self.workflow_detail_state.loading {
                    if matches!(key, KeyCode::Esc | KeyCode::Char('q')) {
                        if let Some(parent) = self.detail_back_stack.pop() {
                            self.load_detail(parent, true);
                        } else {
                            self.current_screen = Screen::Workflows;
                        }
                    }
                    return Ok(());
                }
                // Handle event detail modal scrolling and dismissal
                if self.workflow_detail_state.show_event_detail {
                    match key {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.workflow_detail_state.show_event_detail = false;
                            self.workflow_detail_state.event_detail_lines.clear();
                            self.workflow_detail_state.event_detail_scroll_offset = 0;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.workflow_detail_state.event_detail_scroll_offset = self
                                .workflow_detail_state
                                .event_detail_scroll_offset
                                .saturating_add(1);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.workflow_detail_state.event_detail_scroll_offset = self
                                .workflow_detail_state
                                .event_detail_scroll_offset
                                .saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            self.workflow_detail_state.event_detail_scroll_offset = self
                                .workflow_detail_state
                                .event_detail_scroll_offset
                                .saturating_add(10);
                        }
                        KeyCode::PageUp => {
                            self.workflow_detail_state.event_detail_scroll_offset = self
                                .workflow_detail_state
                                .event_detail_scroll_offset
                                .saturating_sub(10);
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                // Handle success message dismissal - any key dismisses
                if self.workflow_detail_state.success_message.is_some() {
                    self.workflow_detail_state.success_message = None;
                    return Ok(());
                }

                // Handle dialog input mode separately
                if let Some(operation) = &self.workflow_detail_state.show_dialog {
                    match key {
                        KeyCode::Char(c) => {
                            self.workflow_detail_state.dialog_input.push(c);
                        }
                        KeyCode::Backspace => {
                            self.workflow_detail_state.dialog_input.pop();
                        }
                        KeyCode::Enter => {
                            // Execute the operation
                            if let Some(workflow) = &self.workflow_detail_state.dialog_workflow {
                                if let Some(execution) = &workflow.execution {
                                    let workflow_id = execution.workflow_id.clone();
                                    let run_id = execution.run_id.clone();
                                    let input = self.workflow_detail_state.dialog_input.clone();

                                    match operation {
                                        WorkflowOperation::Terminate => {
                                            let reason = if input.is_empty() {
                                                "Terminated by user".to_string()
                                            } else {
                                                input
                                            };
                                            let _ = self.command_tx.send(
                                                AppCommand::TerminateWorkflow(
                                                    workflow_id,
                                                    run_id,
                                                    reason,
                                                ),
                                            );
                                        }
                                        WorkflowOperation::Cancel => {
                                            let _ = self.command_tx.send(
                                                AppCommand::CancelWorkflow(workflow_id, run_id),
                                            );
                                        }
                                        WorkflowOperation::Signal => {
                                            if !input.is_empty() {
                                                let _ = self.command_tx.send(
                                                    AppCommand::SignalWorkflow(
                                                        workflow_id,
                                                        run_id,
                                                        input,
                                                    ),
                                                );
                                            } else {
                                                self.workflow_detail_state.error =
                                                    Some("Signal name cannot be empty".to_string());
                                                self.workflow_detail_state.show_dialog = None;
                                                self.workflow_detail_state.dialog_input.clear();
                                            }
                                        }
                                    }
                                }
                            }
                            // Close dialog after sending command
                            self.workflow_detail_state.show_dialog = None;
                            self.workflow_detail_state.dialog_workflow = None;
                            self.workflow_detail_state.dialog_input.clear();
                        }
                        KeyCode::Esc => {
                            // Cancel dialog
                            self.workflow_detail_state.show_dialog = None;
                            self.workflow_detail_state.dialog_workflow = None;
                            self.workflow_detail_state.dialog_input.clear();
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                // Normal mode key handling
                match key {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if let Some(parent) = self.detail_back_stack.pop() {
                            self.load_detail(parent, true);
                        } else {
                            self.current_screen = Screen::Workflows;
                        }
                    }
                    KeyCode::Char('1') => {
                        self.detail_back_stack.clear();
                        self.current_screen = Screen::Workflows;
                    }
                    KeyCode::Char('2') => {
                        self.current_screen = Screen::Namespaces;
                        // Load namespaces if empty
                        if self.namespace_list_state.items.is_empty()
                            && !self.namespace_list_state.loading
                        {
                            self.namespace_list_state.loading = true;
                            let _ = self.command_tx.send(AppCommand::RefreshNamespaces);
                        }
                    }
                    KeyCode::Char('t') => {
                        // Show terminate dialog
                        let target = if self.workflow_detail_state.show_history {
                            self.workflow_detail_state.workflow.clone()
                        } else {
                            self.workflow_detail_state
                                .selected_outline_workflow()
                                .cloned()
                        };
                        if target.is_none() {
                            return Ok(());
                        }
                        self.workflow_detail_state.dialog_workflow = target;
                        self.workflow_detail_state.show_dialog = Some(WorkflowOperation::Terminate);
                        self.workflow_detail_state.dialog_input.clear();
                        self.workflow_detail_state.success_message = None;
                        self.workflow_detail_state.error = None;
                    }
                    KeyCode::Char('x') => {
                        // Show cancel dialog
                        let target = if self.workflow_detail_state.show_history {
                            self.workflow_detail_state.workflow.clone()
                        } else {
                            self.workflow_detail_state
                                .selected_outline_workflow()
                                .cloned()
                        };
                        if target.is_none() {
                            return Ok(());
                        }
                        self.workflow_detail_state.dialog_workflow = target;
                        self.workflow_detail_state.show_dialog = Some(WorkflowOperation::Cancel);
                        self.workflow_detail_state.dialog_input.clear();
                        self.workflow_detail_state.success_message = None;
                        self.workflow_detail_state.error = None;
                    }
                    KeyCode::Char('s') => {
                        // Show signal dialog
                        let target = if self.workflow_detail_state.show_history {
                            self.workflow_detail_state.workflow.clone()
                        } else {
                            self.workflow_detail_state
                                .selected_outline_workflow()
                                .cloned()
                        };
                        if target.is_none() {
                            return Ok(());
                        }
                        self.workflow_detail_state.dialog_workflow = target;
                        self.workflow_detail_state.show_dialog = Some(WorkflowOperation::Signal);
                        self.workflow_detail_state.dialog_input.clear();
                        self.workflow_detail_state.success_message = None;
                        self.workflow_detail_state.error = None;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.workflow_detail_state.select_next();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.workflow_detail_state.select_previous();
                    }
                    KeyCode::Tab => {
                        self.workflow_detail_state.show_history =
                            !self.workflow_detail_state.show_history;
                    }
                    KeyCode::Char('r') => {
                        if !self.workflow_detail_state.refreshing {
                            if let Some(workflow) = self.workflow_detail_state.workflow.clone() {
                                self.load_detail(workflow, false);
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        self.workflow_detail_state.auto_refresh_enabled =
                            !self.workflow_detail_state.auto_refresh_enabled;
                    }
                    KeyCode::Enter => {
                        if self.workflow_detail_state.show_history {
                            if let Some(event) = self.workflow_detail_state.selected_event() {
                                let lines = structured_attributes(event).unwrap_or_else(|error| {
                                    vec![format!("Could not decode event attributes: {error}")]
                                });
                                self.workflow_detail_state.event_detail_lines = lines;
                                self.workflow_detail_state.event_detail_scroll_offset = 0;
                                self.workflow_detail_state.show_event_detail = true;
                            }
                        } else if let Some(child) = self
                            .workflow_detail_state
                            .selected_outline_workflow()
                            .cloned()
                        {
                            if child.execution
                                != self
                                    .workflow_detail_state
                                    .workflow
                                    .as_ref()
                                    .and_then(|w| w.execution.clone())
                            {
                                if let Some(parent) = self.workflow_detail_state.workflow.clone() {
                                    self.detail_back_stack.push(parent);
                                    self.load_detail(child, true);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Screen::Help => match key {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => {
                    self.current_screen = Screen::Workflows;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_state.scroll_down(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_state.scroll_up(1);
                }
                KeyCode::PageDown => {
                    self.help_state.scroll_down(10);
                }
                KeyCode::PageUp => {
                    self.help_state.scroll_up(10);
                }
                _ => {}
            },
        }
        Ok(())
    }
}

// Note: App is no longer Clone since it owns channels and moves into run()

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_query_combines_search_and_status() {
        let mut state = WorkflowListState::new();
        state.query = "WorkflowType = 'Example'".into();
        state.active_filter = Some(WorkflowFilter::Running);
        assert_eq!(
            state.get_query(),
            "ParentWorkflowId IS NULL AND ExecutionStatus = 'Running' AND (WorkflowType = 'Example')"
        );
    }

    #[test]
    fn parent_scope_defaults_on_and_can_show_all() {
        let mut state = WorkflowListState::new();
        assert_eq!(state.get_query(), "ParentWorkflowId IS NULL");
        state.show_child_workflows = true;
        assert_eq!(state.get_query(), "");
        state.active_filter = Some(WorkflowFilter::Failed);
        assert_eq!(state.get_query(), "ExecutionStatus = 'Failed'");
    }

    #[test]
    fn parent_scope_applies_to_entire_or_search() {
        let mut state = WorkflowListState::new();
        state.query = "WorkflowId = 'parent' OR WorkflowId = 'child'".into();
        assert_eq!(
            state.get_query(),
            "ParentWorkflowId IS NULL AND (WorkflowId = 'parent' OR WorkflowId = 'child')"
        );
    }

    #[test]
    fn pagination_tokens_track_current_page() {
        let mut state = WorkflowListState::new();
        assert_eq!(state.begin_previous_page(), None);
        state.next_page_token = vec![1];
        assert_eq!(state.begin_next_page(), Some(vec![1]));
        assert_eq!(state.current_page, 2);
        assert_eq!(state.begin_next_page(), None); // cannot page while loading
        state.loading = false;
        state.next_page_token = vec![2];
        assert_eq!(state.begin_next_page(), Some(vec![2]));
        assert_eq!(state.current_page, 3);
        state.loading = false;
        assert_eq!(state.begin_previous_page(), Some(vec![1]));
        assert_eq!(state.current_page, 2);
        state.loading = false;
        assert_eq!(state.begin_previous_page(), Some(Vec::new()));
        assert_eq!(state.current_page, 1);
        assert_eq!(state.begin_previous_page(), None);
    }
}
