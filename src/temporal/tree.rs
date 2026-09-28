//! Build a readable execution outline from Temporal visibility and event histories.
use crate::generated::temporal::api::{
    enums::v1::{PendingActivityState, WorkflowExecutionStatus},
    history::v1::{history_event::Attributes, HistoryEvent},
    workflow::v1::{PendingActivityInfo, PendingChildExecutionInfo, WorkflowExecutionInfo},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeStatus {
    Running,
    Queued,
    Paused,
    Canceling,
    Completed,
    Failed,
    Canceled,
    TimedOut,
    Terminated,
    Unknown,
}

impl NodeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "● Running",
            Self::Queued => "◌ Queued",
            Self::Paused => "‖ Paused",
            Self::Canceling => "◌ Canceling",
            Self::Completed => "✓ Done",
            Self::Failed => "✗ Failed",
            Self::Canceled => "– Canceled",
            Self::TimedOut => "! Timed out",
            Self::Terminated => "! Terminated",
            Self::Unknown => "? Unknown",
        }
    }

    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Queued | Self::Canceling)
    }
}

impl From<i32> for NodeStatus {
    fn from(value: i32) -> Self {
        match WorkflowExecutionStatus::try_from(value) {
            Ok(WorkflowExecutionStatus::Running) => Self::Running,
            Ok(WorkflowExecutionStatus::Completed) => Self::Completed,
            Ok(WorkflowExecutionStatus::Failed) => Self::Failed,
            Ok(WorkflowExecutionStatus::Canceled) => Self::Canceled,
            Ok(WorkflowExecutionStatus::Terminated) => Self::Terminated,
            Ok(WorkflowExecutionStatus::TimedOut) => Self::TimedOut,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkflowSnapshot {
    pub info: WorkflowExecutionInfo,
    pub history: Vec<HistoryEvent>,
    pub pending_activities: Vec<PendingActivityInfo>,
    pub pending_children: Vec<PendingChildExecutionInfo>,
}

#[derive(Clone, Debug)]
pub struct OutlineRow {
    pub depth: usize,
    pub label: String,
    pub workflow_id: String,
    pub status: NodeStatus,
    pub workflow: Option<WorkflowExecutionInfo>, // None for activities and not-yet-visible children
    pub is_activity: bool,
}

fn execution_key(info: &WorkflowExecutionInfo) -> Option<(String, String)> {
    info.execution
        .as_ref()
        .map(|e| (e.workflow_id.clone(), e.run_id.clone()))
}

pub fn build_outline(
    snapshots: &[WorkflowSnapshot],
    root: &WorkflowExecutionInfo,
) -> Vec<OutlineRow> {
    let Some(root_key) = execution_key(root) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut visited = HashSet::new();
    visit(&root_key, 0, snapshots, &mut visited, &mut rows);
    rows
}

fn visit(
    key: &(String, String),
    depth: usize,
    snapshots: &[WorkflowSnapshot],
    visited: &mut HashSet<(String, String)>,
    rows: &mut Vec<OutlineRow>,
) {
    if depth > 32 || !visited.insert(key.clone()) {
        return;
    }
    let Some(snapshot) = snapshots
        .iter()
        .find(|s| execution_key(&s.info).as_ref() == Some(key))
    else {
        return;
    };
    let info = &snapshot.info;
    rows.push(OutlineRow {
        depth,
        label: info
            .r#type
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "Workflow".into()),
        workflow_id: key.0.clone(),
        status: info.status.into(),
        workflow: Some(info.clone()),
        is_activity: false,
    });

    // Activity terminal events refer back to their original scheduled event, not the
    // most recent start. Keying by scheduled_event_id also handles retry attempts.
    let mut activity_status: HashMap<i64, NodeStatus> = HashMap::new();
    let mut child_status: HashMap<i64, NodeStatus> = HashMap::new();
    for event in &snapshot.history {
        match &event.attributes {
            Some(Attributes::ActivityTaskScheduledEventAttributes(_)) => {
                activity_status.insert(event.event_id, NodeStatus::Queued);
            }
            Some(Attributes::ActivityTaskStartedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Running);
            }
            Some(Attributes::ActivityTaskCompletedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Completed);
            }
            Some(Attributes::ActivityTaskFailedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Failed);
            }
            Some(Attributes::ActivityTaskTimedOutEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::TimedOut);
            }
            Some(Attributes::ActivityTaskCanceledEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Canceled);
            }
            Some(Attributes::StartChildWorkflowExecutionInitiatedEventAttributes(_)) => {
                child_status.insert(event.event_id, NodeStatus::Queued);
            }
            Some(Attributes::ChildWorkflowExecutionStartedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Running);
            }
            Some(Attributes::ChildWorkflowExecutionCompletedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Completed);
            }
            Some(Attributes::ChildWorkflowExecutionFailedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Failed);
            }
            Some(Attributes::ChildWorkflowExecutionCanceledEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Canceled);
            }
            Some(Attributes::ChildWorkflowExecutionTimedOutEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::TimedOut);
            }
            Some(Attributes::ChildWorkflowExecutionTerminatedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Terminated);
            }
            _ => {}
        }
    }

    // Temporal may defer writing ActivityTaskStarted to history until an attempt finishes.
    // DescribeWorkflowExecution.pending_activities is authoritative for live state.
    let pending_by_id: HashMap<_, _> = snapshot
        .pending_activities
        .iter()
        .map(|a| (a.activity_id.as_str(), a))
        .collect();
    // Walk scheduled events so activities and children appear in *execution order*.
    for event in &snapshot.history {
        match &event.attributes {
            Some(Attributes::ActivityTaskScheduledEventAttributes(a)) => {
                let status = pending_by_id
                    .get(a.activity_id.as_str())
                    .map(|pending| pending_status(pending))
                    .unwrap_or_else(|| {
                        *activity_status
                            .get(&event.event_id)
                            .unwrap_or(&NodeStatus::Unknown)
                    });
                rows.push(OutlineRow {
                    depth: depth + 1,
                    label: a
                        .activity_type
                        .as_ref()
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| "Activity".into()),
                    workflow_id: key.0.clone(),
                    status,
                    workflow: None,
                    is_activity: true,
                });
            }
            Some(Attributes::StartChildWorkflowExecutionInitiatedEventAttributes(a)) => {
                let child = snapshots.iter().find(|s| {
                    s.info
                        .execution
                        .as_ref()
                        .is_some_and(|e| e.workflow_id == a.workflow_id)
                        && s.info
                            .parent_execution
                            .as_ref()
                            .is_some_and(|p| p.workflow_id == key.0 && p.run_id == key.1)
                });
                if let Some(child) = child {
                    if let Some(child_key) = execution_key(&child.info) {
                        visit(&child_key, depth + 1, snapshots, visited, rows);
                    }
                } else {
                    rows.push(OutlineRow {
                        depth: depth + 1,
                        label: a
                            .workflow_type
                            .as_ref()
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "Child workflow".into()),
                        workflow_id: a.workflow_id.clone(),
                        status: if snapshot
                            .pending_children
                            .iter()
                            .any(|child| child.initiated_id == event.event_id)
                        {
                            NodeStatus::Running
                        } else {
                            *child_status
                                .get(&event.event_id)
                                .unwrap_or(&NodeStatus::Queued)
                        },
                        workflow: None,
                        is_activity: false,
                    });
                }
            }
            _ => {}
        }
    }
}

fn pending_status(info: &PendingActivityInfo) -> NodeStatus {
    if info.paused {
        return NodeStatus::Paused;
    }
    match PendingActivityState::try_from(info.state) {
        Ok(PendingActivityState::Started | PendingActivityState::PauseRequested) => {
            NodeStatus::Running
        }
        Ok(PendingActivityState::Scheduled) => NodeStatus::Queued,
        Ok(PendingActivityState::CancelRequested) => NodeStatus::Canceling,
        Ok(PendingActivityState::Paused) => NodeStatus::Paused,
        _ => NodeStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::temporal::api::{
        common::v1::{ActivityType, WorkflowExecution, WorkflowType},
        history::v1::{
            ActivityTaskCompletedEventAttributes, ActivityTaskScheduledEventAttributes,
            ActivityTaskStartedEventAttributes,
            StartChildWorkflowExecutionInitiatedEventAttributes,
        },
    };

    fn info(
        id: &str,
        name: &str,
        parent: Option<WorkflowExecution>,
        status: WorkflowExecutionStatus,
    ) -> WorkflowExecutionInfo {
        WorkflowExecutionInfo {
            execution: Some(WorkflowExecution {
                workflow_id: id.into(),
                run_id: "run".into(),
            }),
            parent_execution: parent,
            r#type: Some(WorkflowType { name: name.into() }),
            status: status as i32,
            ..Default::default()
        }
    }

    #[test]
    fn pending_activity_reports_running_before_start_appears_in_history() {
        let root = info("order", "Checkout", None, WorkflowExecutionStatus::Running);
        let snapshot = WorkflowSnapshot {
            info: root.clone(),
            history: vec![HistoryEvent {
                event_id: 17,
                attributes: Some(Attributes::ActivityTaskScheduledEventAttributes(
                    ActivityTaskScheduledEventAttributes {
                        activity_id: "3".into(),
                        activity_type: Some(ActivityType {
                            name: "AwaitCarrierPickup".into(),
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            }],
            pending_children: vec![],
            pending_activities: vec![PendingActivityInfo {
                activity_id: "3".into(),
                state: PendingActivityState::Started as i32,
                ..Default::default()
            }],
        };
        let rows = build_outline(&[snapshot], &root);
        assert_eq!(rows[1].label, "AwaitCarrierPickup");
        assert_eq!(rows[1].status, NodeStatus::Running);
        assert!(rows[1].status.active());
    }

    #[test]
    fn pending_child_is_running_before_visibility_catches_up() {
        let root = info("order", "Checkout", None, WorkflowExecutionStatus::Running);
        let snapshot = WorkflowSnapshot {
            info: root.clone(),
            history: vec![HistoryEvent {
                event_id: 7,
                attributes: Some(
                    Attributes::StartChildWorkflowExecutionInitiatedEventAttributes(
                        StartChildWorkflowExecutionInitiatedEventAttributes {
                            workflow_id: "shipping".into(),
                            workflow_type: Some(WorkflowType {
                                name: "Shipping".into(),
                            }),
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            }],
            pending_activities: vec![],
            pending_children: vec![PendingChildExecutionInfo {
                initiated_id: 7,
                workflow_id: "shipping".into(),
                ..Default::default()
            }],
        };
        let rows = build_outline(&[snapshot], &root);
        assert_eq!(rows[1].label, "Shipping");
        assert_eq!(rows[1].status, NodeStatus::Running);
    }

    #[test]
    fn nested_workflows_and_activity_state_follow_history() {
        let root = info("order", "Checkout", None, WorkflowExecutionStatus::Running);
        let child = info(
            "payment",
            "Payment",
            root.execution.clone(),
            WorkflowExecutionStatus::Running,
        );
        let grandchild = info(
            "fraud",
            "Fraud",
            child.execution.clone(),
            WorkflowExecutionStatus::Completed,
        );
        let activity = |name: &str, id| HistoryEvent {
            event_id: id,
            attributes: Some(Attributes::ActivityTaskScheduledEventAttributes(
                ActivityTaskScheduledEventAttributes {
                    activity_type: Some(ActivityType { name: name.into() }),
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        let started = |id| HistoryEvent {
            attributes: Some(Attributes::ActivityTaskStartedEventAttributes(
                ActivityTaskStartedEventAttributes {
                    scheduled_event_id: id,
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        let child_event = |id, name: &str, workflow_id: &str| HistoryEvent {
            event_id: id,
            attributes: Some(
                Attributes::StartChildWorkflowExecutionInitiatedEventAttributes(
                    StartChildWorkflowExecutionInitiatedEventAttributes {
                        workflow_id: workflow_id.into(),
                        workflow_type: Some(WorkflowType { name: name.into() }),
                        ..Default::default()
                    },
                ),
            ),
            ..Default::default()
        };
        let snapshots = vec![
            WorkflowSnapshot {
                info: root.clone(),
                pending_activities: vec![],
                pending_children: vec![],
                history: vec![
                    activity("Validate", 5),
                    HistoryEvent {
                        attributes: Some(Attributes::ActivityTaskCompletedEventAttributes(
                            ActivityTaskCompletedEventAttributes {
                                scheduled_event_id: 5,
                                ..Default::default()
                            },
                        )),
                        ..Default::default()
                    },
                    child_event(11, "Payment", "payment"),
                ],
            },
            WorkflowSnapshot {
                info: child,
                pending_activities: vec![],
                pending_children: vec![],
                history: vec![
                    child_event(7, "Fraud", "fraud"),
                    activity("Charge", 9),
                    started(9),
                ],
            },
            WorkflowSnapshot {
                info: grandchild,
                pending_activities: vec![],
                pending_children: vec![],
                history: vec![activity("Risk", 4)],
            },
        ];
        let rows = build_outline(&snapshots, &root);
        assert_eq!(
            rows.iter()
                .map(|r| (r.depth, r.label.as_str(), r.status))
                .collect::<Vec<_>>(),
            vec![
                (0, "Checkout", NodeStatus::Running),
                (1, "Validate", NodeStatus::Completed),
                (1, "Payment", NodeStatus::Running),
                (2, "Fraud", NodeStatus::Completed),
                (3, "Risk", NodeStatus::Queued),
                (2, "Charge", NodeStatus::Running),
            ]
        );
    }
}
