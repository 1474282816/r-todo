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
    pub deleted_at: Option<DateTime<Utc>>,
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
            deleted_at: self
                .deleted_at
                .or_else(|| self.deleted.then_some(self.updated_at)),
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
    if file.format != "rtools/todo" && file.format != "r-todo/todo" {
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

/// 根据当前支持的 RRULE 子集计算下一次实例。
///
/// 桌面编辑器和历史数据使用 DAILY/WEEKLY/MONTHLY、INTERVAL、BYDAY、
/// BYMONTHDAY。其余 RFC 5545 属性会被忽略，避免把未知规则误判成一次性任务。
pub fn next_occurrence(rule: &str, base: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use chrono::{Datelike, Duration, NaiveTime, TimeZone, Timelike, Weekday};
    use std::collections::HashSet;

    let fields: std::collections::HashMap<_, _> = rule
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| {
            (
                key.trim().to_ascii_uppercase(),
                value.trim().to_ascii_uppercase(),
            )
        })
        .collect();
    let frequency = fields.get("FREQ")?.as_str();
    let interval = fields
        .get("INTERVAL")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(1)
        .max(1) as i64;
    let by_days: HashSet<Weekday> = fields
        .get("BYDAY")
        .map(|value| {
            value
                .split(',')
                .filter_map(|day| {
                    match day
                        .trim()
                        .trim_start_matches(['+', '-', '1', '2', '3', '4', '5'])
                    {
                        "MO" => Some(Weekday::Mon),
                        "TU" => Some(Weekday::Tue),
                        "WE" => Some(Weekday::Wed),
                        "TH" => Some(Weekday::Thu),
                        "FR" => Some(Weekday::Fri),
                        "SA" => Some(Weekday::Sat),
                        "SU" => Some(Weekday::Sun),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let by_month_day = fields
        .get("BYMONTHDAY")
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.parse::<u32>().ok());
    let base_day = base.date_naive();
    let base_week = base_day.checked_sub_signed(Duration::days(
        base_day.weekday().num_days_from_monday() as i64,
    ))?;
    for offset in 1..=(366 * 10) {
        let day = base_day.checked_add_signed(Duration::days(offset))?;
        let valid = match frequency {
            "DAILY" => offset % interval == 0,
            "WEEKLY" => {
                let current_week = day.checked_sub_signed(Duration::days(
                    day.weekday().num_days_from_monday() as i64,
                ))?;
                let weeks = (current_week - base_week).num_days() / 7;
                let interval_ok = weeks >= 0 && weeks % interval == 0;
                let day_ok = by_days.is_empty() || by_days.contains(&day.weekday());
                interval_ok && day_ok
            }
            "MONTHLY" => {
                let months = (day.year() - base_day.year()) * 12 + day.month0() as i32
                    - base_day.month0() as i32;
                let interval_ok = months >= 0 && (months as i64) % interval == 0;
                let wanted = by_month_day.unwrap_or(base_day.day());
                interval_ok && day.day() == wanted
            }
            _ => return None,
        };
        if valid {
            let time = NaiveTime::from_hms_opt(base.hour(), base.minute(), base.second())?;
            return Utc
                .from_utc_datetime(&day.and_time(time))
                .with_nanosecond(base.nanosecond());
        }
    }
    None
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
        let template = r#"{
          "format":"FORMAT","version":1,"exported_at":"2026-11-16T00:00:00Z",
          "todos":[{"id":"x","title":"测试","priority":"high","remind_at":null,
          "remind_offset_min":15,"repeat_rule":null,"sort_order":1000,
          "deleted":false,"completed_at":null,
          "created_at":"2026-11-16T00:00:00Z","updated_at":"2026-11-16T00:00:00Z"}]
        }"#;
        for format in ["r-todo/todo", "rtools/todo"] {
            let json = template.replace("FORMAT", format);
            let tasks = parse_rtodo_json(&json).unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].title, "测试");
        }
    }

    #[test]
    fn advances_daily_and_weekly_rules() {
        use chrono::TimeZone;
        let base = Utc.with_ymd_and_hms(2026, 11, 16, 9, 30, 0).unwrap();
        assert_eq!(
            next_occurrence("FREQ=DAILY;INTERVAL=1", base),
            Some(Utc.with_ymd_and_hms(2026, 11, 17, 9, 30, 0).unwrap())
        );
        assert_eq!(
            next_occurrence("FREQ=WEEKLY;INTERVAL=1;BYDAY=MO,WE,FR", base),
            Some(Utc.with_ymd_and_hms(2026, 11, 18, 9, 30, 0).unwrap())
        );
    }
}
