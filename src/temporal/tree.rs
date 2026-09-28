//! Build a readable execution outline from Temporal visibility and event histories.
use crate::generated::temporal::api::{
    enums::v1::{PendingActivityState, WorkflowExecutionStatus},
    history::v1::{history_event::Attributes, HistoryEvent},
    workflow::v1::{PendingActivityInfo, PendingChildExecutionInfo, WorkflowExecutionInfo},
};
use chrono::{DateTime, Utc};
use prost_types::{Duration as ProtoDuration, Timestamp};
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
    pub started_at: Option<Timestamp>,
    pub ended_at: Option<Timestamp>,
    pub reported_duration: Option<ProtoDuration>,
}

impl OutlineRow {
    /// Runtime, not time spent queued. Unknown start/end times stay unknown.
    pub fn runtime_millis(&self, now: DateTime<Utc>) -> Option<u64> {
        if matches!(
            self.status,
            NodeStatus::Queued | NodeStatus::Paused | NodeStatus::Unknown
        ) {
            return None;
        }
        if !self.status.active() {
            if let Some(duration) = &self.reported_duration {
                let seconds = u64::try_from(duration.seconds).ok()?;
                let nanos = u64::try_from(duration.nanos).ok()?;
                return Some(
                    seconds
                        .saturating_mul(1000)
                        .saturating_add(nanos / 1_000_000),
                );
            }
        }
        let start = self
            .started_at
            .as_ref()
            .and_then(|t| DateTime::<Utc>::from_timestamp(t.seconds, t.nanos as u32))?;
        let end = if self.status.active() {
            now
        } else {
            let t = self.ended_at.as_ref()?;
            DateTime::<Utc>::from_timestamp(t.seconds, t.nanos as u32)?
        };
        Some(end.signed_duration_since(start).num_milliseconds().max(0) as u64)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutlineFilter {
    #[default]
    All,
    Active,
    Failed,
    Completed,
}

impl OutlineFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Failed,
            Self::Failed => Self::Completed,
            Self::Completed => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Failed => "Failed",
            Self::Completed => "Done",
        }
    }
    fn matches(self, status: NodeStatus) -> bool {
        match self {
            Self::All => true,
            Self::Active => status.active(),
            Self::Failed => matches!(
                status,
                NodeStatus::Failed | NodeStatus::TimedOut | NodeStatus::Terminated
            ),
            Self::Completed => status == NodeStatus::Completed,
        }
    }
}

/// Keep matching rows *and* their ancestors so a filtered result remains a tree.
pub fn filter_outline(rows: &[OutlineRow], query: &str, filter: OutlineFilter) -> Vec<OutlineRow> {
    let query = query.to_lowercase();
    let mut keep = vec![false; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        let matches_text = query.is_empty()
            || row.label.to_lowercase().contains(&query)
            || row.workflow_id.to_lowercase().contains(&query)
            || row
                .workflow
                .as_ref()
                .and_then(|w| w.execution.as_ref())
                .is_some_and(|e| e.run_id.to_lowercase().contains(&query));
        if !filter.matches(row.status) || !matches_text {
            continue;
        }
        keep[index] = true;
        let mut depth = row.depth;
        for parent in (0..index).rev() {
            if rows[parent].depth < depth {
                keep[parent] = true;
                depth = rows[parent].depth;
                if depth == 0 {
                    break;
                }
            }
        }
    }
    rows.iter()
        .zip(keep)
        .filter_map(|(row, show)| show.then(|| row.clone()))
        .collect()
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
        started_at: info
            .execution_time
            .clone()
            .or_else(|| info.start_time.clone()),
        ended_at: info.close_time.clone(),
        reported_duration: info.execution_duration.clone(),
    });

    // Activity terminal events refer back to their original scheduled event, not the
    // most recent start. Keying by scheduled_event_id also handles retry attempts.
    let mut activity_status: HashMap<i64, NodeStatus> = HashMap::new();
    let mut activity_times: HashMap<i64, (Option<Timestamp>, Option<Timestamp>)> = HashMap::new();
    let mut child_status: HashMap<i64, NodeStatus> = HashMap::new();
    let mut child_times: HashMap<i64, (Option<Timestamp>, Option<Timestamp>)> = HashMap::new();
    for event in &snapshot.history {
        match &event.attributes {
            Some(Attributes::ActivityTaskScheduledEventAttributes(_)) => {
                activity_status.insert(event.event_id, NodeStatus::Queued);
            }
            Some(Attributes::ActivityTaskStartedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Running);
                activity_times.entry(a.scheduled_event_id).or_default().0 =
                    event.event_time.clone();
            }
            Some(Attributes::ActivityTaskCompletedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Completed);
                activity_times.entry(a.scheduled_event_id).or_default().1 =
                    event.event_time.clone();
            }
            Some(Attributes::ActivityTaskFailedEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Failed);
                activity_times.entry(a.scheduled_event_id).or_default().1 =
                    event.event_time.clone();
            }
            Some(Attributes::ActivityTaskTimedOutEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::TimedOut);
                activity_times.entry(a.scheduled_event_id).or_default().1 =
                    event.event_time.clone();
            }
            Some(Attributes::ActivityTaskCanceledEventAttributes(a)) => {
                activity_status.insert(a.scheduled_event_id, NodeStatus::Canceled);
                activity_times.entry(a.scheduled_event_id).or_default().1 =
                    event.event_time.clone();
            }
            Some(Attributes::StartChildWorkflowExecutionInitiatedEventAttributes(_)) => {
                child_status.insert(event.event_id, NodeStatus::Queued);
            }
            Some(Attributes::ChildWorkflowExecutionStartedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Running);
                child_times.entry(a.initiated_event_id).or_default().0 = event.event_time.clone();
            }
            Some(Attributes::ChildWorkflowExecutionCompletedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Completed);
                child_times.entry(a.initiated_event_id).or_default().1 = event.event_time.clone();
            }
            Some(Attributes::ChildWorkflowExecutionFailedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Failed);
                child_times.entry(a.initiated_event_id).or_default().1 = event.event_time.clone();
            }
            Some(Attributes::ChildWorkflowExecutionCanceledEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Canceled);
                child_times.entry(a.initiated_event_id).or_default().1 = event.event_time.clone();
            }
            Some(Attributes::ChildWorkflowExecutionTimedOutEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::TimedOut);
                child_times.entry(a.initiated_event_id).or_default().1 = event.event_time.clone();
            }
            Some(Attributes::ChildWorkflowExecutionTerminatedEventAttributes(a)) => {
                child_status.insert(a.initiated_event_id, NodeStatus::Terminated);
                child_times.entry(a.initiated_event_id).or_default().1 = event.event_time.clone();
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
                    started_at: pending_by_id
                        .get(a.activity_id.as_str())
                        .and_then(|pending| pending.last_started_time.clone())
                        .or_else(|| {
                            activity_times
                                .get(&event.event_id)
                                .and_then(|t| t.0.clone())
                        }),
                    ended_at: activity_times
                        .get(&event.event_id)
                        .and_then(|t| t.1.clone()),
                    reported_duration: None,
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
                        started_at: child_times.get(&event.event_id).and_then(|t| t.0.clone()),
                        ended_at: child_times.get(&event.event_id).and_then(|t| t.1.clone()),
                        reported_duration: None,
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
    fn runtime_uses_execution_and_activity_start_not_queue_time() {
        let mut root = info(
            "order",
            "Checkout",
            None,
            WorkflowExecutionStatus::Completed,
        );
        root.start_time = Some(Timestamp {
            seconds: 90,
            nanos: 0,
        });
        root.execution_time = Some(Timestamp {
            seconds: 100,
            nanos: 0,
        });
        root.close_time = Some(Timestamp {
            seconds: 130,
            nanos: 0,
        });
        let ts = |seconds| Some(Timestamp { seconds, nanos: 0 });
        let history = vec![
            HistoryEvent {
                event_id: 5,
                event_time: ts(102),
                attributes: Some(Attributes::ActivityTaskScheduledEventAttributes(
                    ActivityTaskScheduledEventAttributes {
                        activity_id: "a1".into(),
                        activity_type: Some(ActivityType {
                            name: "Charge".into(),
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            HistoryEvent {
                event_id: 6,
                event_time: ts(110),
                attributes: Some(Attributes::ActivityTaskStartedEventAttributes(
                    ActivityTaskStartedEventAttributes {
                        scheduled_event_id: 5,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            HistoryEvent {
                event_id: 7,
                event_time: ts(114),
                attributes: Some(Attributes::ActivityTaskCompletedEventAttributes(
                    ActivityTaskCompletedEventAttributes {
                        scheduled_event_id: 5,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        ];
        let rows = build_outline(
            &[WorkflowSnapshot {
                info: root.clone(),
                history,
                pending_activities: vec![],
                pending_children: vec![],
            }],
            &root,
        );
        let now = DateTime::<Utc>::from_timestamp(200, 0).unwrap();
        assert_eq!(rows[0].runtime_millis(now), Some(30_000));
        assert_eq!(rows[1].runtime_millis(now), Some(4_000)); // not 12s since scheduled
    }

    #[test]
    fn subsecond_runs_are_not_reported_as_zero_seconds() {
        let row = OutlineRow {
            depth: 1,
            label: "FastActivity".into(),
            workflow_id: "order".into(),
            status: NodeStatus::Completed,
            workflow: None,
            is_activity: true,
            started_at: Some(Timestamp {
                seconds: 100,
                nanos: 0,
            }),
            ended_at: Some(Timestamp {
                seconds: 100,
                nanos: 420_000_000,
            }),
            reported_duration: None,
        };
        let now = DateTime::<Utc>::from_timestamp(200, 0).unwrap();
        assert_eq!(row.runtime_millis(now), Some(420));
    }

    #[test]
    fn live_pending_activity_clock_and_filter_keep_ancestors() {
        let mut root = info("order", "Checkout", None, WorkflowExecutionStatus::Running);
        root.execution_time = Some(Timestamp {
            seconds: 100,
            nanos: 0,
        });
        let snapshot = WorkflowSnapshot {
            info: root.clone(),
            history: vec![HistoryEvent {
                event_id: 5,
                attributes: Some(Attributes::ActivityTaskScheduledEventAttributes(
                    ActivityTaskScheduledEventAttributes {
                        activity_id: "a1".into(),
                        activity_type: Some(ActivityType {
                            name: "WaitForWorker".into(),
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            }],
            pending_activities: vec![PendingActivityInfo {
                activity_id: "a1".into(),
                last_started_time: Some(Timestamp {
                    seconds: 120,
                    nanos: 0,
                }),
                state: PendingActivityState::Started as i32,
                ..Default::default()
            }],
            pending_children: vec![],
        };
        let rows = build_outline(&[snapshot], &root);
        let now = DateTime::<Utc>::from_timestamp(130, 0).unwrap();
        assert_eq!(rows[0].runtime_millis(now), Some(30_000));
        assert_eq!(rows[1].runtime_millis(now), Some(10_000));
        let filtered = filter_outline(&rows, "waitforworker", OutlineFilter::Active);
        assert_eq!(
            filtered
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["Checkout", "WaitForWorker"]
        );
        assert_eq!(
            filter_outline(&rows, "missing", OutlineFilter::All).len(),
            0
        );
        assert_eq!(filter_outline(&rows, "", OutlineFilter::Failed).len(), 0);
        assert_eq!(
            OutlineFilter::All.next().next().next().next(),
            OutlineFilter::All
        );
    }

    #[test]
    fn failed_filter_keeps_ancestors_without_unrelated_siblings() {
        let row = |depth, label: &str, status| OutlineRow {
            depth,
            label: label.into(),
            workflow_id: label.into(),
            status,
            workflow: None,
            is_activity: false,
            started_at: None,
            ended_at: None,
            reported_duration: None,
        };
        let rows = vec![
            row(0, "Root", NodeStatus::Running),
            row(1, "Payment", NodeStatus::Completed),
            row(2, "Charge", NodeStatus::Failed),
            row(1, "Shipping", NodeStatus::Completed),
        ];
        let filtered = filter_outline(&rows, "charge", OutlineFilter::Failed);
        assert_eq!(
            filtered
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["Root", "Payment", "Charge"]
        );
        assert_eq!(filter_outline(&rows, "", OutlineFilter::Failed).len(), 3);
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
