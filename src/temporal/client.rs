use crate::config::{ConnectionProfile, TlsConfig};
use crate::generated::temporal::api::workflowservice::v1::{
    workflow_service_client::WorkflowServiceClient, DescribeWorkflowExecutionRequest,
    GetSystemInfoRequest, GetWorkflowExecutionHistoryRequest, ListNamespacesRequest,
    ListWorkflowExecutionsRequest, RequestCancelWorkflowExecutionRequest,
    SignalWorkflowExecutionRequest, TerminateWorkflowExecutionRequest,
};
use crate::generated::temporal::api::{
    common::v1::WorkflowExecution, enums::v1::HistoryEventFilterType,
};
use anyhow::{Context, Result};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

/// Temporal gRPC client wrapper
pub struct TemporalClient {
    client: WorkflowServiceClient<Channel>,
    namespace: String,
    api_key: Option<MetadataValue<Ascii>>,
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
        tracing::info!(
            "Connecting to Temporal at {} (namespace: {})",
            address,
            namespace
        );

        // Never transmit bearer credentials over an unencrypted connection.
        let use_tls = tls_config.map(|t| t.enabled).unwrap_or(false);
        if api_key.is_some() && !use_tls {
            anyhow::bail!("API key authentication requires TLS (tls.enabled: true)");
        }
        let api_key = api_key
            .map(|key| {
                MetadataValue::try_from(format!("Bearer {}", key))
                    .context("Invalid API key for authorization header")
            })
            .transpose()?;
        if let Some(tls) = tls_config {
            if tls.cert_path.is_some() != tls.key_path.is_some() {
                anyhow::bail!("mTLS requires both cert_path and key_path");
            }
        }

        // Determine if we should use TLS
        let scheme = if use_tls { "https" } else { "http" };

        // Build the endpoint
        let mut endpoint = Endpoint::from_shared(format!("{}://{}", scheme, address))?
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10));

        // Configure TLS if enabled
        if let Some(tls) = tls_config {
            if tls.enabled {
                let mut tls_config = ClientTlsConfig::new().with_native_roots();

                // Load client certificates if provided (mTLS)
                if let (Some(cert_path), Some(key_path)) = (&tls.cert_path, &tls.key_path) {
                    tracing::info!("Configuring mTLS with cert: {:?}", cert_path);
                    let cert = std::fs::read_to_string(cert_path)
                        .context("Failed to read TLS certificate")?;
                    let key =
                        std::fs::read_to_string(key_path).context("Failed to read TLS key")?;

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
        let channel = endpoint
            .connect()
            .await
            .context("Failed to connect to Temporal server")?;

        // Create client
        let mut client = WorkflowServiceClient::new(channel);

        if api_key.is_some() {
            tracing::info!("Using API key authentication");
        }

        // Verify connection with a health check
        let mut health_request = tonic::Request::new(GetSystemInfoRequest {});
        if let Some(ref key) = api_key {
            health_request
                .metadata_mut()
                .insert("authorization", key.clone());
        }
        client
            .get_system_info(health_request)
            .await
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
            request.metadata_mut().insert("authorization", key.clone());
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

    /// Read authoritative execution state, including activities currently running in workers.
    pub async fn describe_workflow_execution(
        &mut self,
        workflow_id: &str,
        run_id: &str,
    ) -> Result<
        crate::generated::temporal::api::workflowservice::v1::DescribeWorkflowExecutionResponse,
    > {
        let request = self.add_api_key(tonic::Request::new(DescribeWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            execution: Some(WorkflowExecution {
                workflow_id: workflow_id.to_owned(),
                run_id: run_id.to_owned(),
            }),
        }));
        Ok(self
            .client
            .describe_workflow_execution(request)
            .await?
            .into_inner())
    }

    pub async fn get_workflow_execution_history(
        &mut self,
        workflow_id: String,
        run_id: String,
        page_size: i32,
        next_page_token: Vec<u8>,
    ) -> Result<
        crate::generated::temporal::api::workflowservice::v1::GetWorkflowExecutionHistoryResponse,
    > {
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

    /// Read every history page for one execution (including empty histories).
    pub async fn history_events(
        &mut self,
        workflow_id: &str,
        run_id: &str,
    ) -> Result<Vec<crate::generated::temporal::api::history::v1::HistoryEvent>> {
        let mut events = Vec::new();
        let mut token = Vec::new();
        loop {
            let response = self
                .get_workflow_execution_history(
                    workflow_id.to_owned(),
                    run_id.to_owned(),
                    200,
                    token.clone(),
                )
                .await?;
            if let Some(history) = response.history {
                events.extend(history.events);
            }
            if response.next_page_token.is_empty() {
                return Ok(events);
            }
            if response.next_page_token == token {
                anyhow::bail!("History pagination did not advance");
            }
            token = response.next_page_token;
        }
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
        let request =
            self.add_api_key(tonic::Request::new(RequestCancelWorkflowExecutionRequest {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_key_requires_tls() {
        let error = TemporalClient::connect(
            "localhost:7233".into(),
            "default".into(),
            None,
            Some("secret".into()),
        )
        .await
        .err()
        .expect("insecure API key must be rejected");
        assert!(error.to_string().contains("requires TLS"));
    }

    #[tokio::test]
    async fn incomplete_mtls_is_rejected() {
        let tls = TlsConfig {
            enabled: true,
            cert_path: Some("client.pem".into()),
            key_path: None,
            ca_path: None,
        };
        let error =
            TemporalClient::connect("localhost:7233".into(), "default".into(), Some(&tls), None)
                .await
                .err()
                .expect("incomplete mTLS must be rejected");
        assert!(error.to_string().contains("both cert_path and key_path"));
    }
}
