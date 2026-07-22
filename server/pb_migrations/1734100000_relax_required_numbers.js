/// <reference path="../pb_data/types.d.ts" />

/**
 * 把 sort_order 和 remind_offset_min 改成 required: false。
 *
 * 原因：PocketBase 的 required number 字段收到 0 会被 ozzo Required 规则
 * 当作"空值"拒绝（HTTP 400 validation_required）。而 0 对这两个字段都是
 * 合法值（sort_order 顶部任务 = 0；remind_offset_min = 0 表示准点提醒）。
 */
migrate((db) => {
    const dao = new Dao(db);
    const collection = dao.findCollectionByNameOrId("tasks");

    const sortOrder = collection.schema.getFieldByName("sort_order");
    if (sortOrder) sortOrder.required = false;

    const remindOffset = collection.schema.getFieldByName("remind_offset_min");
    if (remindOffset) remindOffset.required = false;

    return dao.saveCollection(collection);
}, (db) => {
    const dao = new Dao(db);
    const collection = dao.findCollectionByNameOrId("tasks");

    const sortOrder = collection.schema.getFieldByName("sort_order");
    if (sortOrder) sortOrder.required = true;

    const remindOffset = collection.schema.getFieldByName("remind_offset_min");
    if (remindOffset) remindOffset.required = true;

    return dao.saveCollection(collection);
});
