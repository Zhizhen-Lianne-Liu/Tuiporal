use crate::config::Config;
use crate::events::{Event, EventHandler};
use crate::generated::temporal::api::{
    history::v1::HistoryEvent,
    workflowservice::v1::DescribeNamespaceResponse,
    workflow::v1::WorkflowExecutionInfo,
};
use crate::temporal::TemporalClient;
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
    RefreshWorkflows(String), // query
    LoadNextPage(String, Vec<u8>), // query, page_token
    LoadPreviousPage(String), // query - will start fresh and rebuild
    ViewWorkflowDetail(String, String), // workflow_id, run_id
    RefreshNamespaces,
    SwitchNamespace(String),
    TerminateWorkflow(String, String, String), // workflow_id, run_id, reason
    CancelWorkflow(String, String),             // workflow_id, run_id
    SignalWorkflow(String, String, String),     // workflow_id, run_id, signal_name
}

/// Results from async operations
#[derive(Debug, Clone)]
pub enum AppResult {
    WorkflowsLoaded {
        workflows: Vec<WorkflowExecutionInfo>,
        next_page_token: Vec<u8>,
    },
    WorkflowsError(String),
    WorkflowDetailLoaded {
        workflow: WorkflowExecutionInfo,
        history: Vec<HistoryEvent>,
    },
    WorkflowDetailError(String),
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
    pub prev_page_tokens: Vec<Vec<u8>>, // Stack of previous page tokens
    pub loading: bool,
    pub error: Option<String>,
    pub current_page: usize,
    pub query: String,
    pub query_history: Vec<String>,
    pub input_mode: bool,
    pub active_filter: Option<WorkflowFilter>,
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
            loading: false,
            error: None,
            current_page: 1,
            query: String::new(),
            query_history: Vec::new(),
            input_mode: false,
            active_filter: None,
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
            queries.push(self.query.clone());
        }

        queries.join(" AND ")
    }

    pub fn has_next_page(&self) -> bool {
        !self.next_page_token.is_empty()
    }

    pub fn has_prev_page(&self) -> bool {
        !self.prev_page_tokens.is_empty()
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
        self.table_state
            .selected()
            .and_then(|i| self.items.get(i))
    }
}

/// State for the workflow detail screen
#[derive(Debug, Clone)]
pub struct WorkflowDetailState {
    pub workflow: Option<WorkflowExecutionInfo>,
    pub history: Vec<HistoryEvent>,
    pub table_state: TableState,
    pub loading: bool,
    pub error: Option<String>,
    pub show_dialog: Option<WorkflowOperation>,
    pub dialog_input: String,
    pub success_message: Option<String>,
    pub show_event_detail: bool,
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
            table_state: TableState::default(),
            loading: false,
            error: None,
            show_dialog: None,
            dialog_input: String::new(),
            success_message: None,
            show_event_detail: false,
            event_detail_scroll_offset: 0,
        }
    }

    pub fn selected_event(&self) -> Option<&HistoryEvent> {
        self.table_state
            .selected()
            .and_then(|i| self.history.get(i))
    }

    pub fn select_next(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.history.len() - 1 {
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
        if self.history.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.history.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
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
        self.table_state
            .selected()
            .and_then(|i| self.items.get(i))
    }
}

/// State for the help screen
#[derive(Debug, Clone)]
pub struct HelpState {
    pub scroll_offset: u16,
}

impl HelpState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
        }
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
    pub async fn new() -> Result<Self> {
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
            command_tx,
            result_rx,
        };

        // Connect to Temporal
        app.connect_temporal().await?;

        // Spawn async task handler
        if let Some(client) = app.client.take() {
            app.spawn_task_handler(client, command_rx, result_tx);
        }

        // Load initial workflow list
        app.command_tx.send(AppCommand::RefreshWorkflows(String::new()))?;

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
                    AppCommand::RefreshWorkflows(query) => {
                        tracing::info!("Loading workflows with query: '{}'", query);
                        match client
                            .list_workflow_executions(50, Vec::new(), query)
                            .await
                        {
                            Ok(response) => {
                                let _ = result_tx.send(AppResult::WorkflowsLoaded {
                                    workflows: response.executions,
                                    next_page_token: response.next_page_token,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx
                                    .send(AppResult::WorkflowsError(format!("Failed to load workflows: {}", e)));
                            }
                        }
                    }
                    AppCommand::LoadNextPage(query, page_token) => {
                        tracing::info!("Loading next page with query: '{}'", query);
                        match client
                            .list_workflow_executions(50, page_token, query)
                            .await
                        {
                            Ok(response) => {
                                let _ = result_tx.send(AppResult::WorkflowsLoaded {
                                    workflows: response.executions,
                                    next_page_token: response.next_page_token,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx
                                    .send(AppResult::WorkflowsError(format!("Failed to load next page: {}", e)));
                            }
                        }
                    }
                    AppCommand::LoadPreviousPage(query) => {
                        tracing::info!("Loading previous page with query: '{}'", query);
                        // Load from the beginning (previous page is handled on the client side)
                        match client
                            .list_workflow_executions(50, Vec::new(), query)
                            .await
                        {
                            Ok(response) => {
                                let _ = result_tx.send(AppResult::WorkflowsLoaded {
                                    workflows: response.executions,
                                    next_page_token: response.next_page_token,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx
                                    .send(AppResult::WorkflowsError(format!("Failed to load previous page: {}", e)));
                            }
                        }
                    }
                    AppCommand::ViewWorkflowDetail(workflow_id, run_id) => {
                        tracing::info!("Loading workflow detail: {}", workflow_id);

                        // First, find the workflow in our list
                        let workflow_info = client
                            .list_workflow_executions(1, Vec::new(), format!("WorkflowId = '{}'", workflow_id))
                            .await
                            .ok()
                            .and_then(|response| response.executions.into_iter().next());

                        // Get the history
                        match client
                            .get_workflow_execution_history(workflow_id.clone(), run_id, 100, Vec::new())
                            .await
                        {
                            Ok(response) => {
                                if let Some(history) = response.history {
                                    let _ = result_tx.send(AppResult::WorkflowDetailLoaded {
                                        workflow: workflow_info.unwrap_or_default(),
                                        history: history.events,
                                    });
                                }
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowDetailError(
                                    format!("Failed to load workflow detail: {}", e),
                                ));
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
                                let _ = result_tx.send(AppResult::NamespacesError(
                                    format!("Failed to load namespaces: {}", e),
                                ));
                            }
                        }
                    }
                    AppCommand::SwitchNamespace(namespace) => {
                        tracing::info!("Switching to namespace: {}", namespace);
                        client.set_namespace(namespace.clone());
                        let _ = result_tx.send(AppResult::NamespaceSwitched { namespace });
                    }
                    AppCommand::TerminateWorkflow(workflow_id, run_id, reason) => {
                        tracing::info!("Terminating workflow: {} with reason: {}", workflow_id, reason);
                        match client.terminate_workflow(workflow_id.clone(), run_id, reason).await {
                            Ok(_) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationSuccess(
                                    format!("Workflow {} terminated successfully", workflow_id),
                                ));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(
                                    format!("Failed to terminate workflow: {}", e),
                                ));
                            }
                        }
                    }
                    AppCommand::CancelWorkflow(workflow_id, run_id) => {
                        tracing::info!("Canceling workflow: {}", workflow_id);
                        match client.cancel_workflow(workflow_id.clone(), run_id).await {
                            Ok(_) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationSuccess(
                                    format!("Workflow {} cancel requested successfully", workflow_id),
                                ));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(
                                    format!("Failed to cancel workflow: {}", e),
                                ));
                            }
                        }
                    }
                    AppCommand::SignalWorkflow(workflow_id, run_id, signal_name) => {
                        tracing::info!("Signaling workflow: {} with signal: {}", workflow_id, signal_name);
                        match client.signal_workflow(workflow_id.clone(), run_id, signal_name.clone()).await {
                            Ok(_) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationSuccess(
                                    format!("Signal '{}' sent to workflow {} successfully", signal_name, workflow_id),
                                ));
                            }
                            Err(e) => {
                                let _ = result_tx.send(AppResult::WorkflowOperationError(
                                    format!("Failed to signal workflow: {}", e),
                                ));
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

    fn process_results(&mut self) {
        // Process all available results from async tasks
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                AppResult::WorkflowsLoaded {
                    workflows,
                    next_page_token,
                } => {
                    self.workflow_list_state.items = workflows;
                    self.workflow_list_state.next_page_token = next_page_token;
                    self.workflow_list_state.loading = false;
                    self.workflow_list_state.error = None;
                    self.workflow_list_state.mark_refreshed();

                    // Select first item if list is not empty
                    if !self.workflow_list_state.items.is_empty() {
                        self.workflow_list_state.table_state.select(Some(0));
                    }

                    tracing::info!("Loaded {} workflows (page {})",
                                   self.workflow_list_state.items.len(),
                                   self.workflow_list_state.current_page);
                }
                AppResult::WorkflowsError(error) => {
                    self.workflow_list_state.error = Some(error.clone());
                    self.workflow_list_state.loading = false;
                    tracing::error!("{}", error);
                }
                AppResult::WorkflowDetailLoaded { workflow, history } => {
                    self.workflow_detail_state.workflow = Some(workflow);
                    self.workflow_detail_state.history = history;
                    self.workflow_detail_state.loading = false;
                    self.workflow_detail_state.error = None;

                    // Select first event if list is not empty
                    if !self.workflow_detail_state.history.is_empty()
                        && self.workflow_detail_state.table_state.selected().is_none()
                    {
                        self.workflow_detail_state.table_state.select(Some(0));
                    }

                    tracing::info!("Loaded {} history events", self.workflow_detail_state.history.len());
                }
                AppResult::WorkflowDetailError(error) => {
                    self.workflow_detail_state.error = Some(error.clone());
                    self.workflow_detail_state.loading = false;
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

                    tracing::info!("Loaded {} namespaces", self.namespace_list_state.items.len());
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
                    self.workflow_list_state.loading = true;
                    let query = self.workflow_list_state.get_query();
                    let _ = self.command_tx.send(AppCommand::RefreshWorkflows(query));
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
            if matches!(self.current_screen, Screen::Workflows) && self.workflow_list_state.should_refresh() {
                tracing::debug!("Auto-refreshing workflows");
                self.workflow_list_state.loading = true;
                let query = self.workflow_list_state.get_query();
                let _ = self.command_tx.send(AppCommand::RefreshWorkflows(query));
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
                                self.workflow_list_state.query_history.push(self.workflow_list_state.query.clone());
                            }
                            // Exit input mode and refresh (reset to page 1)
                            self.workflow_list_state.input_mode = false;
                            self.workflow_list_state.loading = true;
                            self.workflow_list_state.prev_page_tokens.clear();
                            self.workflow_list_state.current_page = 1;
                            let query = self.workflow_list_state.get_query();
                            let _ = self.command_tx.send(AppCommand::RefreshWorkflows(query));
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
                        if self.namespace_list_state.items.is_empty() && !self.namespace_list_state.loading {
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
                        self.workflow_list_state.active_filter = match self.workflow_list_state.active_filter {
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
                        self.workflow_list_state.current_page = 1;
                        let query = self.workflow_list_state.get_query();
                        let _ = self.command_tx.send(AppCommand::RefreshWorkflows(query));
                    }
                    KeyCode::Char('c') => {
                        // Clear filter and search (reset to page 1)
                        self.workflow_list_state.active_filter = None;
                        self.workflow_list_state.query.clear();
                        self.workflow_list_state.loading = true;
                        self.workflow_list_state.prev_page_tokens.clear();
                        self.workflow_list_state.current_page = 1;
                        let _ = self.command_tx.send(AppCommand::RefreshWorkflows(String::new()));
                    }
                    KeyCode::Char('a') => {
                        // Toggle auto-refresh
                        self.workflow_list_state.auto_refresh_enabled = !self.workflow_list_state.auto_refresh_enabled;
                        if self.workflow_list_state.auto_refresh_enabled {
                            tracing::info!("Auto-refresh enabled ({}s interval)", self.workflow_list_state.auto_refresh_interval_secs);
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
                        self.workflow_list_state.current_page = 1;
                        let query = self.workflow_list_state.get_query();
                        let _ = self.command_tx.send(AppCommand::RefreshWorkflows(query));
                    }
                    KeyCode::Char('n') | KeyCode::Right => {
                        // Next page
                        if self.workflow_list_state.has_next_page() && !self.workflow_list_state.loading {
                            tracing::info!("Loading next page");
                            self.workflow_list_state.loading = true;

                            // Save current page token for going back
                            if !self.workflow_list_state.next_page_token.is_empty() {
                                // We're about to go forward, so save where we are
                                // This is a bit tricky: we need to track the token that got us TO this page
                                // For simplicity, we'll rebuild previous pages by using the page number
                                self.workflow_list_state.prev_page_tokens.push(Vec::new()); // Placeholder
                                self.workflow_list_state.current_page += 1;
                            }

                            let query = self.workflow_list_state.get_query();
                            let page_token = self.workflow_list_state.next_page_token.clone();
                            let _ = self.command_tx.send(AppCommand::LoadNextPage(query, page_token));
                        }
                    }
                    KeyCode::Char('p') | KeyCode::Left => {
                        // Previous page
                        if self.workflow_list_state.has_prev_page() && !self.workflow_list_state.loading {
                            tracing::info!("Loading previous page");
                            self.workflow_list_state.loading = true;

                            // Pop the last page token
                            self.workflow_list_state.prev_page_tokens.pop();
                            self.workflow_list_state.current_page = self.workflow_list_state.current_page.saturating_sub(1).max(1);

                            let query = self.workflow_list_state.get_query();
                            let _ = self.command_tx.send(AppCommand::LoadPreviousPage(query));
                        }
                    }
                    KeyCode::Enter => {
                        // View workflow detail
                        if let Some(workflow) = self.workflow_list_state.selected_workflow() {
                            if let Some(execution) = &workflow.execution {
                                tracing::info!("Viewing workflow: {}", execution.workflow_id);
                                self.workflow_detail_state.loading = true;
                                let _ = self.command_tx.send(AppCommand::ViewWorkflowDetail(
                                    execution.workflow_id.clone(),
                                    execution.run_id.clone(),
                                ));
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
                            let _ = self.command_tx.send(AppCommand::SwitchNamespace(namespace_name));
                        }
                    }
                }
                _ => {}
            },
            Screen::WorkflowDetail => {
                // Handle event detail modal scrolling and dismissal
                if self.workflow_detail_state.show_event_detail {
                    match key {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.workflow_detail_state.show_event_detail = false;
                            self.workflow_detail_state.event_detail_scroll_offset = 0;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.workflow_detail_state.event_detail_scroll_offset =
                                self.workflow_detail_state.event_detail_scroll_offset.saturating_add(1);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.workflow_detail_state.event_detail_scroll_offset =
                                self.workflow_detail_state.event_detail_scroll_offset.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            self.workflow_detail_state.event_detail_scroll_offset =
                                self.workflow_detail_state.event_detail_scroll_offset.saturating_add(10);
                        }
                        KeyCode::PageUp => {
                            self.workflow_detail_state.event_detail_scroll_offset =
                                self.workflow_detail_state.event_detail_scroll_offset.saturating_sub(10);
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
                            if let Some(workflow) = &self.workflow_detail_state.workflow {
                                if let Some(execution) = &workflow.execution {
                                    let workflow_id = execution.workflow_id.clone();
                                    let run_id = execution.run_id.clone();
                                    let input = self.workflow_detail_state.dialog_input.clone();

                                    match operation {
                                        WorkflowOperation::Terminate => {
                                            let reason = if input.is_empty() { "Terminated by user".to_string() } else { input };
                                            let _ = self.command_tx.send(AppCommand::TerminateWorkflow(workflow_id, run_id, reason));
                                        }
                                        WorkflowOperation::Cancel => {
                                            let _ = self.command_tx.send(AppCommand::CancelWorkflow(workflow_id, run_id));
                                        }
                                        WorkflowOperation::Signal => {
                                            if !input.is_empty() {
                                                let _ = self.command_tx.send(AppCommand::SignalWorkflow(workflow_id, run_id, input));
                                            } else {
                                                self.workflow_detail_state.error = Some("Signal name cannot be empty".to_string());
                                                self.workflow_detail_state.show_dialog = None;
                                                self.workflow_detail_state.dialog_input.clear();
                                            }
                                        }
                                    }
                                }
                            }
                            // Close dialog after sending command
                            self.workflow_detail_state.show_dialog = None;
                            self.workflow_detail_state.dialog_input.clear();
                        }
                        KeyCode::Esc => {
                            // Cancel dialog
                            self.workflow_detail_state.show_dialog = None;
                            self.workflow_detail_state.dialog_input.clear();
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                // Normal mode key handling
                match key {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        self.current_screen = Screen::Workflows;
                    }
                    KeyCode::Char('1') => {
                        self.current_screen = Screen::Workflows;
                    }
                    KeyCode::Char('2') => {
                        self.current_screen = Screen::Namespaces;
                        // Load namespaces if empty
                        if self.namespace_list_state.items.is_empty() && !self.namespace_list_state.loading {
                            self.namespace_list_state.loading = true;
                            let _ = self.command_tx.send(AppCommand::RefreshNamespaces);
                        }
                    }
                    KeyCode::Char('t') => {
                        // Show terminate dialog
                        self.workflow_detail_state.show_dialog = Some(WorkflowOperation::Terminate);
                        self.workflow_detail_state.dialog_input.clear();
                        self.workflow_detail_state.success_message = None;
                        self.workflow_detail_state.error = None;
                    }
                    KeyCode::Char('x') => {
                        // Show cancel dialog
                        self.workflow_detail_state.show_dialog = Some(WorkflowOperation::Cancel);
                        self.workflow_detail_state.dialog_input.clear();
                        self.workflow_detail_state.success_message = None;
                        self.workflow_detail_state.error = None;
                    }
                    KeyCode::Char('s') => {
                        // Show signal dialog
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
                    KeyCode::Enter => {
                        // Show event detail modal
                        if self.workflow_detail_state.selected_event().is_some() {
                            self.workflow_detail_state.event_detail_scroll_offset = 0;
                            self.workflow_detail_state.show_event_detail = true;
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
