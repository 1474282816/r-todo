/// <reference path="../pb_data/types.d.ts" />

/**
 * 初始化 `tasks` collection.
 *
 * PocketBase 0.22.x 语法（JSVM）：用 new Collection() + Dao(db).saveCollection()
 * 字段列表基于 packages/schema/task.schema.json + 客户端 v2 migration (sort_order)
 */
migrate((db) => {
    const collection = new Collection({
        id: "r_todo_tasks_v1",
        type: "base",
        name: "tasks",
        system: false,
        schema: [
            { name: "title",             type: "text",   required: true,  options: { max: 500 } },
            { name: "priority",          type: "select", required: true,  options: { maxSelect: 1, values: ["high", "medium", "low"] } },
            { name: "remind_at",         type: "date",   required: false, options: {} },
            { name: "remind_offset_min", type: "number", required: false, options: { min: 0, noDecimal: true } },
            { name: "repeat_rule",       type: "text",   required: false, options: { max: 500 } },
            { name: "sort_order",        type: "number", required: false, options: {} },
            { name: "completed_at",      type: "date",   required: false, options: {} },
            { name: "user",              type: "relation", required: true, options: { collectionId: "_pb_users_auth_", cascadeDelete: true, maxSelect: 1 } },
        ],
        indexes: [
            "CREATE INDEX idx_tasks_user_completed ON tasks (user, completed_at)",
            "CREATE INDEX idx_tasks_user_updated ON tasks (user, updated)",
        ],
        listRule:   "@request.auth.id != \"\" && user = @request.auth.id",
        viewRule:   "@request.auth.id != \"\" && user = @request.auth.id",
        createRule: "@request.auth.id != \"\" && user = @request.auth.id",
        updateRule: "@request.auth.id != \"\" && user = @request.auth.id",
        deleteRule: "@request.auth.id != \"\" && user = @request.auth.id",
    });

    return Dao(db).saveCollection(collection);
}, (db) => {
    const dao = new Dao(db);
    const collection = dao.findCollectionByNameOrId("tasks");
    return dao.deleteCollection(collection);
});
