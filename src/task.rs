//! User task types.

use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::asana::DataRequest;

/// An Asana workspace.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Workspace {
    /// Workspace GID.
    pub gid: String,
    /// Workspace name.
    pub name: String,
}

impl DataRequest<'_> for Workspace {
    type RequestData = ();
    type ResponseData = Vec<Self>;

    fn segments((): &Self::RequestData) -> Vec<String> {
        vec!["workspaces".to_string()]
    }

    fn fields() -> &'static [&'static str] {
        &["this.gid", "this.name"]
    }
}

/// An Asana project.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Project {
    /// Project GID.
    pub gid: String,
    /// Project name.
    pub name: String,
}

impl<'a> DataRequest<'a> for Project {
    type RequestData = String; // workspace GID
    type ResponseData = Vec<Self>;

    fn segments(workspace_gid: &'a Self::RequestData) -> Vec<String> {
        vec![
            "workspaces".to_string(),
            workspace_gid.clone(),
            "projects".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid", "this.name"]
    }

    fn params(_workspace_gid: &'a Self::RequestData) -> Vec<(&'a str, String)> {
        // Limit results and only get active projects
        vec![
            ("limit", "100".to_string()),
            ("archived", "false".to_string()),
        ]
    }
}

/// A user's task from Asana.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserTask {
    /// Task GID.
    pub gid: String,
    /// When the task was created.
    #[serde(with = "crate::asana::serde_formats::datetime")]
    pub created_at: DateTime<Local>,
    /// When the task is due.
    #[serde(with = "crate::asana::serde_formats::optional_date")]
    pub due_on: Option<NaiveDate>,
    /// Task name.
    pub name: String,
}

impl<'a> DataRequest<'a> for UserTask {
    type RequestData = String;
    type ResponseData = Vec<Self>;

    fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
        vec![
            "user_task_lists".to_string(),
            request_data.clone(),
            "tasks".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid", "this.created_at", "this.due_on", "this.name"]
    }

    fn params(_request_data: &'a Self::RequestData) -> Vec<(&'a str, String)> {
        vec![("completed_since", "now".to_string())]
    }
}

/// Request data for getting a user's task list.
pub struct UserTaskListRequest {
    /// User GID (or "me").
    pub user_gid: String,
    /// Workspace GID.
    pub workspace_gid: String,
}

/// A user's task list reference.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserTaskList {
    /// Task list GID.
    pub gid: String,
}

impl<'a> DataRequest<'a> for UserTaskList {
    type RequestData = UserTaskListRequest;
    type ResponseData = Self;

    fn segments(request: &'a Self::RequestData) -> Vec<String> {
        vec![
            "users".to_string(),
            request.user_gid.clone(),
            "user_task_list".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid"]
    }

    fn params(request: &'a Self::RequestData) -> Vec<(&'a str, String)> {
        vec![("workspace", request.workspace_gid.clone())]
    }
}

/// Request body for creating a new task.
#[derive(Clone, Debug, Serialize)]
pub struct CreateTaskRequest {
    /// Task name/title.
    pub name: String,
    /// Assignee (use "me" for current user).
    pub assignee: String,
    /// Workspace GID.
    pub workspace: String,
    /// Due date (optional).
    #[serde(
        with = "crate::asana::serde_formats::optional_date",
        skip_serializing_if = "Option::is_none"
    )]
    pub due_on: Option<NaiveDate>,
    /// Task notes/description (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
