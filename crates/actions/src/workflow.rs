//! Serde structs mirroring a subset of the GitHub Actions workflow YAML schema.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ActionsError;

/// Top level GitHub Actions workflow document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: Option<String>,
    #[serde(rename = "on")]
    pub on: TriggerConfig,
    pub jobs: HashMap<String, Job>,
}

impl Workflow {
    /// Parse a workflow YAML document.
    pub fn parse(yaml: &str) -> Result<Workflow, ActionsError> {
        let workflow: Workflow =
            serde_yaml::from_str(yaml).map_err(ActionsError::Parse)?;
        Ok(workflow)
    }
}

/// Filters for a single event (branches/tags/paths).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventFilter {
    #[serde(default)]
    pub branches: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(rename = "branches-ignore", default)]
    pub branches_ignore: Option<Vec<String>>,
    #[serde(rename = "tags-ignore", default)]
    pub tags_ignore: Option<Vec<String>>,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

/// An event trigger value: either absent (null / empty map -> any event),
/// a bare list of branch names (shorthand), or a full filter map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventTrigger {
    List(Vec<String>),
    Filter(EventFilter),
    Empty(Option<serde_yaml::Value>),
}

/// The `on:` section of a workflow. GitHub Actions allows `on` to be a bare
/// string, a list of event names, or a map of event name -> filter. We model
/// the common map form here, with each known event optional.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TriggerConfig {
    /// `on: push` or `on: [push, pull_request]`
    Simple(SimpleTrigger),
    /// `on:\n  push:\n    branches: [main]`
    Map(TriggerMap),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SimpleTrigger {
    Single(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerMap {
    #[serde(default)]
    pub push: Option<EventTrigger>,
    #[serde(default)]
    pub pull_request: Option<EventTrigger>,
    #[serde(default)]
    pub workflow_dispatch: Option<EventTrigger>,
    /// Catch-all for any other event names we don't specifically model.
    #[serde(flatten)]
    pub other: HashMap<String, serde_yaml::Value>,
}

/// A single job within a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    #[serde(rename = "runs-on")]
    pub runs_on: String,
    #[serde(default)]
    pub needs: Option<Vec<String>>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

/// A single step within a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub uses: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub with: Option<HashMap<String, serde_yaml::Value>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(rename = "continue-on-error", default)]
    pub continue_on_error_flag: Option<bool>,
}

impl Step {
    /// Whether this step is marked with `continue-on-error: true`. GitHub
    /// Actions models this as a top-level step key; we also accept it inside
    /// `with` for extra tolerance.
    pub fn continue_on_error(&self) -> bool {
        if self.continue_on_error_flag.unwrap_or(false) {
            return true;
        }
        self.with
            .as_ref()
            .and_then(|w| w.get("continue-on-error"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}
