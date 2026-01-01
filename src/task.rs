//! User task types.

use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::asana::DataRequest;

/// Asana workspace GID.
pub const ASANA_WORKSPACE_GID: &str = "1199118829113557";

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

    fn params() -> Vec<(&'a str, String)> {
        vec![("completed_since", "now".to_string())]
    }
}

/// A user's task list reference.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserTaskList {
    /// Task list GID.
    pub gid: String,
}

impl<'a> DataRequest<'a> for UserTaskList {
    type RequestData = String;
    type ResponseData = Self;

    fn segments(user_gid: &'a Self::RequestData) -> Vec<String> {
        vec![
            "users".to_string(),
            user_gid.clone(),
            "user_task_list".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid"]
    }

    fn params() -> Vec<(&'a str, String)> {
        vec![("workspace", ASANA_WORKSPACE_GID.to_string())]
    }
}
