use crate::config::{ConnectionProfile, TlsConfig};
use crate::generated::temporal::api::workflowservice::v1::{
    workflow_service_client::WorkflowServiceClient, GetSystemInfoRequest,
    GetWorkflowExecutionHistoryRequest, ListNamespacesRequest, ListWorkflowExecutionsRequest,
    TerminateWorkflowExecutionRequest, RequestCancelWorkflowExecutionRequest,
    SignalWorkflowExecutionRequest,
};
use crate::generated::temporal::api::{common::v1::WorkflowExecution, enums::v1::HistoryEventFilterType};
use anyhow::{Context, Result};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::metadata::MetadataValue;

/// Temporal gRPC client wrapper
pub struct TemporalClient {
    client: WorkflowServiceClient<Channel>,
    namespace: String,
    api_key: Option<String>,
}

impl TemporalClient {
    /// Create a new Temporal client from a connection profile
    pub async fn from_profile(profile: &ConnectionProfile) -> Result<Self> {
        Self::connect(
            profile.address.clone(),
            profile.namespace.clone(),
            profile.tls.as_ref(),
            profile.api_key.clone(),
        )
        .await
    }

    /// Create a new Temporal client and connect to the server
    pub async fn connect(
        address: String,
        namespace: String,
        tls_config: Option<&TlsConfig>,
        api_key: Option<String>,
    ) -> Result<Self> {
        tracing::info!("Connecting to Temporal at {} (namespace: {})", address, namespace);

        // Determine if we should use TLS
        let use_tls = tls_config.map(|t| t.enabled).unwrap_or(false);
        let scheme = if use_tls { "https" } else { "http" };

        // Build the endpoint
        let mut endpoint = Endpoint::from_shared(format!("{}://{}", scheme, address))?
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10));

        // Configure TLS if enabled
        if let Some(tls) = tls_config {
            if tls.enabled {
                let mut tls_config = ClientTlsConfig::new();

                // Load client certificates if provided (mTLS)
                if let (Some(cert_path), Some(key_path)) = (&tls.cert_path, &tls.key_path) {
                    tracing::info!("Configuring mTLS with cert: {:?}", cert_path);
                    let cert = std::fs::read_to_string(cert_path)
                        .context("Failed to read TLS certificate")?;
                    let key = std::fs::read_to_string(key_path)
                        .context("Failed to read TLS key")?;

                    let identity = tonic::transport::Identity::from_pem(cert, key);
                    tls_config = tls_config.identity(identity);
                }

                // Load CA certificate if provided
                if let Some(ca_path) = &tls.ca_path {
                    tracing::info!("Using custom CA certificate: {:?}", ca_path);
                    let ca = std::fs::read_to_string(ca_path)
                        .context("Failed to read CA certificate")?;
                    let ca_cert = tonic::transport::Certificate::from_pem(ca);
                    tls_config = tls_config.ca_certificate(ca_cert);
                }

                endpoint = endpoint.tls_config(tls_config)?;
            }
        }

        // Connect to the server
        let channel = endpoint.connect().await
            .context("Failed to connect to Temporal server")?;

        // Create client
        let mut client = WorkflowServiceClient::new(channel);

        if api_key.is_some() {
            tracing::info!("Using API key authentication");
        }

        // Verify connection with a health check
        let mut health_request = tonic::Request::new(GetSystemInfoRequest {});
        if let Some(ref key) = api_key {
            let key_value = format!("Bearer {}", key);
            if let Ok(value) = MetadataValue::try_from(&key_value) {
                health_request.metadata_mut().insert("authorization", value);
            }
        }
        client.get_system_info(health_request).await
            .context("Health check failed - unable to connect to Temporal")?;

        tracing::info!("Successfully connected to Temporal");

        Ok(Self {
            client,
            namespace,
            api_key,
        })
    }

    /// Helper method to add API key to requests
    fn add_api_key<T>(&self, mut request: tonic::Request<T>) -> tonic::Request<T> {
        if let Some(ref key) = self.api_key {
            let key_value = format!("Bearer {}", key);
            if let Ok(value) = MetadataValue::try_from(&key_value) {
                request.metadata_mut().insert("authorization", value);
            }
        }
        request
    }

    /// Get system information (health check)
    pub async fn get_system_info(&mut self) -> Result<()> {
        let request = self.add_api_key(tonic::Request::new(GetSystemInfoRequest {}));
        let response = self.client.get_system_info(request).await?;
        let info = response.into_inner();

        tracing::debug!("Server version: {:?}", info.server_version);
        Ok(())
    }

    /// List workflow executions in the current namespace
    pub async fn list_workflow_executions(
        &mut self,
        page_size: i32,
        next_page_token: Vec<u8>,
        query: String,
    ) -> Result<crate::generated::temporal::api::workflowservice::v1::ListWorkflowExecutionsResponse>
    {
        let request = self.add_api_key(tonic::Request::new(ListWorkflowExecutionsRequest {
            namespace: self.namespace.clone(),
            page_size,
            next_page_token,
            query,
        }));

        let response = self.client.list_workflow_executions(request).await?;
        Ok(response.into_inner())
    }

    /// Get workflow execution history
    pub async fn get_workflow_execution_history(
        &mut self,
        workflow_id: String,
        run_id: String,
        page_size: i32,
        next_page_token: Vec<u8>,
    ) -> Result<crate::generated::temporal::api::workflowservice::v1::GetWorkflowExecutionHistoryResponse>
    {
        let request = self.add_api_key(tonic::Request::new(GetWorkflowExecutionHistoryRequest {
            namespace: self.namespace.clone(),
            execution: Some(WorkflowExecution {
                workflow_id,
                run_id,
            }),
            maximum_page_size: page_size,
            next_page_token,
            wait_new_event: false,
            history_event_filter_type: HistoryEventFilterType::AllEvent as i32,
            skip_archival: false,
        }));

        let response = self.client.get_workflow_execution_history(request).await?;
        Ok(response.into_inner())
    }

    /// List all namespaces
    pub async fn list_namespaces(
        &mut self,
        page_size: i32,
        next_page_token: Vec<u8>,
    ) -> Result<crate::generated::temporal::api::workflowservice::v1::ListNamespacesResponse> {
        let request = self.add_api_key(tonic::Request::new(ListNamespacesRequest {
            page_size,
            next_page_token,
            ..Default::default()
        }));

        let response = self.client.list_namespaces(request).await?;
        Ok(response.into_inner())
    }

    /// Get the current namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Switch to a different namespace
    pub fn set_namespace(&mut self, namespace: String) {
        self.namespace = namespace;
    }

    /// Terminate a workflow execution
    pub async fn terminate_workflow(
        &mut self,
        workflow_id: String,
        run_id: String,
        reason: String,
    ) -> Result<()> {
        let request = self.add_api_key(tonic::Request::new(TerminateWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_execution: Some(WorkflowExecution {
                workflow_id,
                run_id,
            }),
            reason,
            ..Default::default()
        }));

        self.client.terminate_workflow_execution(request).await?;
        Ok(())
    }

    /// Request cancellation of a workflow execution
    pub async fn cancel_workflow(&mut self, workflow_id: String, run_id: String) -> Result<()> {
        let request = self.add_api_key(tonic::Request::new(RequestCancelWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_execution: Some(WorkflowExecution {
                workflow_id,
                run_id,
            }),
            ..Default::default()
        }));

        self.client
            .request_cancel_workflow_execution(request)
            .await?;
        Ok(())
    }

    /// Signal a workflow execution
    pub async fn signal_workflow(
        &mut self,
        workflow_id: String,
        run_id: String,
        signal_name: String,
    ) -> Result<()> {
        let request = self.add_api_key(tonic::Request::new(SignalWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_execution: Some(WorkflowExecution {
                workflow_id,
                run_id,
            }),
            signal_name,
            ..Default::default()
        }));

        self.client.signal_workflow_execution(request).await?;
        Ok(())
    }
}
