//! SQLite migrations for tauri-plugin-sql
//!
//! Migration schema **必须与 packages/schema/task.schema.json 对齐**。

use tauri_plugin_sql::{Migration, MigrationKind};

pub fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "create_tasks_table",
            sql: r#"
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK(length(title) > 0),
    priority TEXT NOT NULL CHECK(priority IN ('high', 'medium', 'low')),
    remind_at TEXT,
    remind_offset_min INTEGER NOT NULL DEFAULT 15,
    repeat_rule TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed_at);
CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);
"#,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "add_sort_order_column",
            sql: r#"
ALTER TABLE tasks ADD COLUMN sort_order REAL NOT NULL DEFAULT 0;
UPDATE tasks SET sort_order = rowid * 1000.0;
CREATE INDEX IF NOT EXISTS idx_tasks_sort ON tasks(sort_order);
"#,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 3,
            description: "add_deleted_and_pending_ops",
            sql: r#"
ALTER TABLE tasks ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN deleted_at TEXT;
CREATE INDEX IF NOT EXISTS idx_tasks_deleted ON tasks(deleted);

CREATE TABLE IF NOT EXISTS pending_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    op_type TEXT NOT NULL CHECK(op_type IN ('create', 'update', 'delete')),
    collection TEXT NOT NULL DEFAULT 'tasks',
    record_id TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX IF NOT EXISTS idx_pending_ops_fifo ON pending_ops(id);
"#,
            kind: MigrationKind::Up,
        },
    ]
}
