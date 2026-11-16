use chrono::{Duration, Utc};
use rtools_domain::{Priority, Task, TaskDraft};
use rtools_engine::{db, tasks::TaskService};
use tempfile::tempdir;

fn task(draft: TaskDraft) -> Task {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::migrate(&conn).unwrap();
    TaskService::new(&conn).create(draft).unwrap()
}

#[test]
fn completing_repeating_task_creates_next_instance() {
    let dir = tempdir().unwrap();
    let conn = db::open(&dir.path().join("engine.db")).unwrap();
    let base = Utc::now() + Duration::days(1);
    let created = TaskService::new(&conn)
        .create(TaskDraft {
            title: "每日站会".into(),
            priority: Priority::High,
            remind_at: Some(base),
            remind_offset_min: 15,
            repeat_rule: Some("FREQ=DAILY;INTERVAL=1".into()),
        })
        .unwrap();

    let (done, next) = TaskService::new(&conn)
        .complete_and_roll(&created.id, true)
        .unwrap();

    assert!(done.completed_at.is_some());
    let next = next.expect("下一次实例");
    assert_eq!(next.title, created.title);
    assert!(next.completed_at.is_none());
    assert_eq!(
        next.remind_at.unwrap(),
        created.remind_at.unwrap() + Duration::days(1)
    );
}

#[test]
fn due_reminder_is_taken_once_and_completed_task_is_skipped() {
    let dir = tempdir().unwrap();
    let conn = db::open(&dir.path().join("engine.db")).unwrap();
    let task = TaskService::new(&conn)
        .create(TaskDraft {
            title: "提醒".into(),
            priority: Priority::Medium,
            remind_at: Some(Utc::now() + Duration::minutes(1)),
            remind_offset_min: 15,
            repeat_rule: None,
        })
        .unwrap();

    let due = db::take_due_reminder(&conn, Utc::now()).unwrap().unwrap();
    assert_eq!(due.task_id, task.id);
    assert!(db::take_due_reminder(&conn, Utc::now()).unwrap().is_none());
}
