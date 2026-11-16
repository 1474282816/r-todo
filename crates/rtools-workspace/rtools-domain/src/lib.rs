use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub remind_at: Option<DateTime<Utc>>,
    #[serde(default = "default_offset")]
    pub remind_offset_min: i64,
    #[serde(default)]
    pub repeat_rule: Option<String>,
    #[serde(default)]
    pub sort_order: f64,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub revision: i64,
}

fn default_offset() -> i64 {
    15
}

impl Task {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(ValidationError::EmptyTitle);
        }
        if title.chars().count() > 500 {
            return Err(ValidationError::TitleTooLong);
        }
        if self.remind_offset_min < 0 {
            return Err(ValidationError::NegativeReminderOffset);
        }
        Ok(())
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("任务标题不能为空")]
    EmptyTitle,
    #[error("任务标题不能超过 500 个字符")]
    TitleTooLong,
    #[error("提醒提前量不能为负数")]
    NegativeReminderOffset,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub priority: Option<Priority>,
    pub remind_at: Option<Option<DateTime<Utc>>>,
    pub remind_offset_min: Option<i64>,
    pub repeat_rule: Option<Option<String>>,
    pub sort_order: Option<f64>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl TaskPatch {
    pub fn apply(self, task: &mut Task, now: DateTime<Utc>) {
        if let Some(v) = self.title {
            task.title = v;
        }
        if let Some(v) = self.priority {
            task.priority = v;
        }
        if let Some(v) = self.remind_at {
            task.remind_at = v;
        }
        if let Some(v) = self.remind_offset_min {
            task.remind_offset_min = v;
        }
        if let Some(v) = self.repeat_rule {
            task.repeat_rule = v;
        }
        if let Some(v) = self.sort_order {
            task.sort_order = v;
        }
        if let Some(v) = self.completed_at {
            task.completed_at = v;
        }
        if let Some(v) = self.deleted_at {
            task.deleted_at = v;
        }
        task.updated_at = self.updated_at.unwrap_or(now);
        task.revision = task.revision.saturating_add(1);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDraft {
    pub title: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub remind_at: Option<DateTime<Utc>>,
    #[serde(default = "default_offset")]
    pub remind_offset_min: i64,
    #[serde(default)]
    pub repeat_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RTodoJsonFile {
    pub format: String,
    pub version: u32,
    pub exported_at: Option<DateTime<Utc>>,
    pub todos: Vec<ImportedTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub remind_at: Option<DateTime<Utc>>,
    #[serde(default = "default_offset")]
    pub remind_offset_min: i64,
    #[serde(default)]
    pub repeat_rule: Option<String>,
    #[serde(default)]
    pub sort_order: f64,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ImportedTask {
    pub fn into_task(self) -> Task {
        Task {
            id: self.id,
            title: self.title,
            priority: self.priority,
            remind_at: self.remind_at,
            remind_offset_min: self.remind_offset_min,
            repeat_rule: self.repeat_rule,
            sort_order: self.sort_order,
            completed_at: self.completed_at,
            deleted_at: self.deleted.then_some(self.updated_at),
            created_at: self.created_at,
            updated_at: self.updated_at,
            revision: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportIssue {
    pub index: usize,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub total: usize,
    pub insert: usize,
    pub update: usize,
    pub skip: usize,
    pub deleted: usize,
    pub issues: Vec<ImportIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
}

pub fn parse_rtodo_json(content: &str) -> Result<Vec<Task>, String> {
    let file: RTodoJsonFile =
        serde_json::from_str(content).map_err(|e| format!("JSON 解析失败: {e}"))?;
    if file.format != "r-todo/todo" {
        return Err(format!("不支持的 format: {}", file.format));
    }
    if file.version != 1 {
        return Err(format!("不支持的 version: {}", file.version));
    }
    let mut ids = std::collections::HashSet::new();
    let mut tasks = Vec::with_capacity(file.todos.len());
    for (index, item) in file.todos.into_iter().enumerate() {
        if !ids.insert(item.id.clone()) {
            return Err(format!("第 {} 条任务 ID 重复: {}", index + 1, item.id));
        }
        let task = item.into_task();
        task.validate()
            .map_err(|e| format!("第 {} 条任务无效: {e}", index + 1))?;
        tasks.push(task);
    }
    Ok(tasks)
}

pub fn plan_import(
    incoming: &[Task],
    existing: &std::collections::HashMap<String, Task>,
) -> (ImportPreview, Vec<Task>, Vec<Task>, Vec<Task>) {
    let mut to_insert = Vec::new();
    let mut to_update = Vec::new();
    let mut to_skip = Vec::new();
    let mut issues = Vec::new();
    let mut deleted = 0;
    for (index, task) in incoming.iter().enumerate() {
        if let Err(e) = task.validate() {
            issues.push(ImportIssue {
                index,
                field: "task".into(),
                message: e.to_string(),
            });
            continue;
        }
        if task.is_deleted() {
            deleted += 1;
        }
        match existing.get(&task.id) {
            None => to_insert.push(task.clone()),
            Some(local) if task.updated_at > local.updated_at => to_update.push(task.clone()),
            Some(_) => to_skip.push(task.clone()),
        }
    }
    let preview = ImportPreview {
        total: incoming.len(),
        insert: to_insert.len(),
        update: to_update.len(),
        skip: to_skip.len(),
        deleted,
        issues,
    };
    (preview, to_insert, to_update, to_skip)
}

pub fn sort_active(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        a.is_completed()
            .cmp(&b.is_completed())
            .then_with(|| {
                if a.is_completed() {
                    b.completed_at.cmp(&a.completed_at)
                } else {
                    a.sort_order
                        .partial_cmp(&b.sort_order)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            })
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(title: &str) -> Task {
        let now = Utc::now();
        Task {
            id: "a".into(),
            title: title.into(),
            priority: Priority::Medium,
            remind_at: None,
            remind_offset_min: 15,
            repeat_rule: None,
            sort_order: 0.0,
            completed_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            revision: 0,
        }
    }

    #[test]
    fn validates_title_and_offset() {
        assert_eq!(task("  ").validate(), Err(ValidationError::EmptyTitle));
        let mut t = task("ok");
        t.remind_offset_min = -1;
        assert_eq!(t.validate(), Err(ValidationError::NegativeReminderOffset));
    }

    #[test]
    fn parses_legacy_json() {
        let json = r#"{
          "format":"r-todo/todo","version":1,"exported_at":"2026-11-16T00:00:00Z",
          "todos":[{"id":"x","title":"测试","priority":"high","remind_at":null,
          "remind_offset_min":15,"repeat_rule":null,"sort_order":1000,
          "deleted":false,"completed_at":null,
          "created_at":"2026-11-16T00:00:00Z","updated_at":"2026-11-16T00:00:00Z"}]
        }"#;
        let tasks = parse_rtodo_json(json).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "测试");
    }
}
