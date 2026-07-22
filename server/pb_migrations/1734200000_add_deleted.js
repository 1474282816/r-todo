/// <reference path="../pb_data/types.d.ts" />

/**
 * 为 tasks collection 添加 `deleted` 和 `deleted_at` 字段（软删除 / tombstone）。
 */
migrate((db) => {
    const dao = new Dao(db);
    const collection = dao.findCollectionByNameOrId("tasks");

    const f1 = new SchemaField();
    f1.name = "deleted";
    f1.type = "bool";
    f1.required = false;
    collection.schema.addField(f1);

    const f2 = new SchemaField();
    f2.name = "deleted_at";
    f2.type = "date";
    f2.required = false;
    collection.schema.addField(f2);

    return dao.saveCollection(collection);
}, (db) => {});