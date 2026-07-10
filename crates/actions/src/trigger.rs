//! Event trigger matching against a workflow's `on:` configuration.

use crate::workflow::{EventFilter, EventTrigger, SimpleTrigger, TriggerConfig};

/// Returns true if `event_name` (e.g. "push", "pull_request",
/// "workflow_dispatch") fired on `branch` matches the workflow's trigger
/// configuration.
pub fn matches_event(trigger: &TriggerConfig, event_name: &str, branch: &str) -> bool {
    match trigger {
        TriggerConfig::Simple(simple) => match simple {
            SimpleTrigger::Single(name) => name == event_name,
            SimpleTrigger::Many(names) => names.iter().any(|n| n == event_name),
        },
        TriggerConfig::Map(map) => {
            let event = match event_name {
                "push" => &map.push,
                "pull_request" => &map.pull_request,
                "workflow_dispatch" => &map.workflow_dispatch,
                other => {
                    // Unrecognized event names: match if present in the
                    // catch-all map at all (best-effort compatibility).
                    return map.other.contains_key(other);
                }
            };
            match event {
                None => false,
                Some(EventTrigger::Empty(_)) => true,
                Some(EventTrigger::List(branches)) => branch_matches(branches, &[], branch),
                Some(EventTrigger::Filter(filter)) => filter_matches(filter, branch),
            }
        }
    }
}

fn filter_matches(filter: &EventFilter, branch: &str) -> bool {
    if let Some(ignore) = &filter.branches_ignore {
        if glob_any(ignore, branch) {
            return false;
        }
    }
    if let Some(tags_ignore) = &filter.tags_ignore {
        if glob_any(tags_ignore, branch) {
            return false;
        }
    }

    let has_include = filter.branches.is_some() || filter.tags.is_some();
    if !has_include {
        return true;
    }

    let branches = filter.branches.clone().unwrap_or_default();
    let tags = filter.tags.clone().unwrap_or_default();
    branch_matches(&branches, &tags, branch)
}

fn branch_matches(branches: &[String], tags: &[String], branch: &str) -> bool {
    if branches.is_empty() && tags.is_empty() {
        return true;
    }
    glob_any(branches, branch) || glob_any(tags, branch)
}

/// Very small glob matcher supporting a single trailing `*` wildcard, which
/// covers the overwhelming majority of real-world workflow branch filters
/// (e.g. `release/*`).
fn glob_any(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|p| glob_match(p, value))
}

fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "**" || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return value
            .strip_prefix(prefix)
            .map(|rest| rest.starts_with('/'))
            .unwrap_or(false);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}
