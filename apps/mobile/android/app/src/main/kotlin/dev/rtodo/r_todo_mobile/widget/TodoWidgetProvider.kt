package dev.rtodo.r_todo_mobile.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.database.sqlite.SQLiteDatabase
import android.view.View
import android.widget.RemoteViews
import dev.rtodo.r_todo_mobile.R
import es.antonborri.home_widget.HomeWidgetProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Base64
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/**
 * 桌面小组件：显示未完成任务 + 点击勾选完成（原生直写 SQLite，无需打开 App）。
 *
 * 勾选链路：
 *   点击勾选圈 → PendingIntent 广播（ACTION_TASK_DONE + task id）
 *   → onReceive → 更新 tasks.completed_at/updated_at
 *   → pending_ops 入队（与 Dart 侧 _toRemote 格式一致，等待同步）
 *   → 从 HomeWidgetPreferences 移除该任务 → updateAppWidget 刷新
 */
open class TodoWidgetProvider : HomeWidgetProvider() {
  companion object {
    const val ACTION_TASK_DONE = "dev.rtodo.action.TASK_DONE"
    const val EXTRA_TASK_ID = "task_id"

    /** Flutter shared_preferences 插件的存储文件名与 key 前缀 */
    private const val FLUTTER_PREFS = "FlutterSharedPreferences"
    private const val FLUTTER_PREFIX = "flutter."
    private const val SYNC_AUTH_KEY = "r-todo/sync/auth"

    /** home_widget 插件的存储文件名 */
    private const val WIDGET_PREFS = "HomeWidgetPreferences"
    private const val WIDGET_TASKS_KEY = "tasks"
  }

  override fun onUpdate(
      context: Context,
      appWidgetManager: AppWidgetManager,
      appWidgetIds: IntArray,
      widgetData: SharedPreferences,
  ) {
    try {
      val tasksJson = widgetData.getString(WIDGET_TASKS_KEY, "[]") ?: "[]"
      val tasks = JSONArray(tasksJson)

      appWidgetIds.forEach { widgetId ->
        val views = RemoteViews(context.packageName, R.layout.todo_widget_layout)
        val count = tasks.length()

        // 按 widget 实际高度决定显示行数（行高约 21dp，3x2≈145dp 容纳 5 行）
        val heightDp = appWidgetManager
            .getAppWidgetOptions(widgetId)
            .getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, 110)
        val maxRows = when {
          heightDp >= 130 -> 5 // 3x2 及以上：5 行，行距舒适
          heightDp >= 110 -> 3 // 2x2（110dp）：紧凑 3 行
          else -> 2            // 更小
        }
        val shown = minOf(count, maxRows)

        views.setTextViewText(R.id.widget_title, "rtools")
        views.setTextViewText(R.id.widget_count, "剩余 $count 项")

        val taskIds = intArrayOf(R.id.task_1, R.id.task_2, R.id.task_3, R.id.task_4, R.id.task_5, R.id.task_6)
        val textIds = intArrayOf(R.id.task_text_1, R.id.task_text_2, R.id.task_text_3, R.id.task_text_4, R.id.task_text_5, R.id.task_text_6)
        val priorityIds = intArrayOf(R.id.priority_1, R.id.priority_2, R.id.priority_3, R.id.priority_4, R.id.priority_5, R.id.priority_6)
        val checkIds = intArrayOf(R.id.check_1, R.id.check_2, R.id.check_3, R.id.check_4, R.id.check_5, R.id.check_6)

        for (i in 0 until 6) {
          if (i < shown) {
            val task = tasks.getJSONObject(i)
            views.setTextViewText(textIds[i], task.getString("title"))
            views.setViewVisibility(taskIds[i], View.VISIBLE)
            val colorRes = when (task.getString("priority")) {
              "high" -> R.drawable.priority_high
              "medium" -> R.drawable.priority_medium
              else -> R.drawable.priority_low
            }
            views.setInt(priorityIds[i], "setBackgroundResource", colorRes)
            views.setViewVisibility(priorityIds[i], View.VISIBLE)

            // 勾选圈：点击后原生写库并刷新，无需打开 App
            views.setImageViewResource(checkIds[i], R.drawable.circle_outline)
            views.setViewVisibility(checkIds[i], View.VISIBLE)
            val pi = PendingIntent.getBroadcast(
                context,
                task.getString("id").hashCode(),
                Intent(context, TodoWidgetProvider::class.java)
                    .setAction(ACTION_TASK_DONE)
                    .putExtra(EXTRA_TASK_ID, task.getString("id")),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            views.setOnClickPendingIntent(checkIds[i], pi)
          } else {
            views.setViewVisibility(taskIds[i], View.GONE)
            views.setViewVisibility(priorityIds[i], View.GONE)
            views.setViewVisibility(checkIds[i], View.GONE)
          }
        }

        if (shown == 0) {
          views.setTextViewText(R.id.widget_empty, "今天没有任务")
          views.setViewVisibility(R.id.widget_empty, View.VISIBLE)
        } else {
          views.setViewVisibility(R.id.widget_empty, View.GONE)
        }

        appWidgetManager.updateAppWidget(widgetId, views)
      }
    } catch (e: Exception) {
      e.printStackTrace()
    }
  }

  override fun onAppWidgetOptionsChanged(
      context: Context,
      appWidgetManager: AppWidgetManager,
      appWidgetId: Int,
      newOptions: android.os.Bundle?,
  ) {
    // 用户拖拽改尺寸后按新高度刷新行数
    val prefs = context.getSharedPreferences(WIDGET_PREFS, Context.MODE_PRIVATE)
    onUpdate(context, appWidgetManager, intArrayOf(appWidgetId), prefs)
  }

  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action == ACTION_TASK_DONE) {
      val taskId = intent.getStringExtra(EXTRA_TASK_ID) ?: return
      markTaskDone(context, taskId)
      return
    }
    super.onReceive(context, intent)
  }

  /** 勾选完成：原生直写 SQLite + 同步队列 + 更新 widget 数据并刷新 */
  private fun markTaskDone(context: Context, taskId: String) {
    try {
      val now = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'", Locale.US).apply {
        timeZone = TimeZone.getTimeZone("UTC")
      }.format(Date())

      // drift 的库文件位于 app_flutter/r-todo.db（path_provider 的 getApplicationDocumentsDirectory
      // 在 Android 上 = getDir("flutter") → /data/user/0/<pkg>/app_flutter；注意 getDir 自带 "app_" 前缀）
      val dbFile = File(context.getDir("flutter", Context.MODE_PRIVATE), "r-todo.db")
      SQLiteDatabase.openDatabase(dbFile.absolutePath, null, SQLiteDatabase.OPEN_READWRITE).use { db ->
        db.execSQL(
            "UPDATE tasks SET completed_at = ?, updated_at = ? WHERE id = ?",
            arrayOf(now, now, taskId),
        )

        // 构造同步 payload（字段与 Dart 侧 _toRemote 一致），入 pending_ops 等待推送
        val userId = jwtSubject(
            context.getSharedPreferences(FLUTTER_PREFS, Context.MODE_PRIVATE)
                .getString(FLUTTER_PREFIX + SYNC_AUTH_KEY, null),
        ) ?: ""
        val payload = db.rawQuery(
            "SELECT id, title, priority, remind_at, remind_offset_min, repeat_rule, sort_order, deleted, created_at, updated_at FROM tasks WHERE id = ?",
            arrayOf(taskId),
        ).use { c ->
          if (c.moveToFirst()) {
            JSONObject()
                .put("id", c.getString(0))
                .put("user", userId)
                .put("title", c.getString(1))
                .put("priority", c.getString(2))
                .put("remind_at", c.getString(3) ?: "")
                .put("remind_offset_min", c.getInt(4))
                .put("repeat_rule", c.getString(5) ?: "")
                .put("sort_order", c.getDouble(6))
                .put("deleted", c.getInt(7) != 0)
                .put("completed_at", now)
                .toString()
          } else null
        }
        if (payload != null) {
          db.execSQL(
              "INSERT INTO pending_ops(op_type, collection, record_id, payload, created_at) VALUES('update', 'tasks', ?, ?, ?)",
              arrayOf<Any?>(taskId, payload, System.currentTimeMillis()),
          )
        }
      }

      // 从 widget 数据里移除已勾选任务并刷新
      val wprefs = context.getSharedPreferences(WIDGET_PREFS, Context.MODE_PRIVATE)
      val arr = JSONArray(wprefs.getString(WIDGET_TASKS_KEY, "[]") ?: "[]")
      val out = JSONArray()
      for (i in 0 until arr.length()) {
        if (arr.getJSONObject(i).getString("id") != taskId) out.put(arr.getJSONObject(i))
      }
      wprefs.edit().putString(WIDGET_TASKS_KEY, out.toString()).apply()

      val manager = AppWidgetManager.getInstance(context)
      onUpdate(context, manager, allVariantIds(context), wprefs)
    } catch (e: Exception) {
      e.printStackTrace()
    }
  }

  /** 所有尺寸变体（含基类自身） */
  private fun allVariants(): List<Class<*>> = listOf(
      TodoWidgetProvider::class.java,
      TodoWidget2x2Provider::class.java,
      TodoWidget4x2Provider::class.java,
      TodoWidget4x3Provider::class.java,
  )

  /** 汇总所有变体的 widget 实例 id（勾选/尺寸变化后全部刷新） */
  private fun allVariantIds(context: Context): IntArray {
    val manager = AppWidgetManager.getInstance(context)
    val ids = mutableListOf<Int>()
    for (cls in allVariants()) {
      for (id in manager.getAppWidgetIds(ComponentName(context, cls))) ids.add(id)
    }
    return ids.toIntArray()
  }

  /** 从 PocketBase JWT 解出用户 id（同步 payload 需要） */
  private fun jwtSubject(token: String?): String? {
    if (token == null) return null
    return try {
      val parts = token.split(".")
      if (parts.size < 2) return null
      val payload = Base64.getUrlDecoder().decode(parts[1])
      JSONObject(String(payload, Charsets.UTF_8)).optString("sub").ifEmpty { null }
    } catch (_: Exception) {
      null
    }
  }
}
