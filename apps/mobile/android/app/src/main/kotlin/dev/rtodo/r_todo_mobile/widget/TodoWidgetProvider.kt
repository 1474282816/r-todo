package dev.rtodo.r_todo_mobile.widget

import android.appwidget.AppWidgetManager
import android.content.Context
import android.content.SharedPreferences
import android.graphics.Color
import android.view.View
import android.widget.RemoteViews
import dev.rtodo.r_todo_mobile.R
import es.antonborri.home_widget.HomeWidgetProvider
import org.json.JSONArray

class TodoWidgetProvider : HomeWidgetProvider() {

  override fun onUpdate(
      context: Context,
      appWidgetManager: AppWidgetManager,
      appWidgetIds: IntArray,
      widgetData: SharedPreferences,
  ) {
    try {
      val tasksJson = widgetData.getString("tasks", "[]") ?: "[]"
      val tasks = JSONArray(tasksJson)

      appWidgetIds.forEach { widgetId ->
        val views = RemoteViews(context.packageName, R.layout.todo_widget_layout)
        val count = tasks.length()
        views.setTextViewText(R.id.widget_title, "R待办")
        views.setTextViewText(R.id.widget_count, "剩余 $count 项")

        val taskIds = intArrayOf(R.id.task_1, R.id.task_2, R.id.task_3, R.id.task_4, R.id.task_5, R.id.task_6)
        val priorityIds = intArrayOf(R.id.priority_1, R.id.priority_2, R.id.priority_3, R.id.priority_4, R.id.priority_5, R.id.priority_6)

        for (i in 0 until 6) {
          if (i < count) {
            val task = tasks.getJSONObject(i)
            views.setTextViewText(taskIds[i], task.getString("title"))
            views.setViewVisibility(taskIds[i], View.VISIBLE)
            val color = when (task.getString("priority")) {
              "high" -> Color.parseColor("#EF4444")
              "medium" -> Color.parseColor("#F59E0B")
              else -> Color.parseColor("#9CA3AF")
            }
            views.setInt(priorityIds[i], "setBackgroundColor", color)
            views.setViewVisibility(priorityIds[i], View.VISIBLE)
          } else {
            views.setViewVisibility(taskIds[i], View.GONE)
            views.setViewVisibility(priorityIds[i], View.GONE)
          }
        }

        if (count == 0) {
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
}