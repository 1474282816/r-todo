package dev.rtodo.r_todo_mobile.transfer

import android.app.NotificationChannel
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import dev.rtodo.r_todo_mobile.MainActivity
import dev.rtodo.r_todo_mobile.R

/**
 * 传输期间的前台服务（Android 14+ dataSync 类型）。
 * 只在真正传文件时启动，结束/失败/取消立刻停止。
 */
class TransferForegroundService : Service() {

    companion object {
        private const val TAG = "RtodoTransferService"
        private const val CHANNEL_ID = "rtodo_transfer"
        private const val CHANNEL_NAME = "文件互传"
        private const val NOTIFICATION_ID = 53317

        private const val ACTION_START = "dev.rtodo.r_todo_mobile.transfer.START"
        private const val ACTION_STOP = "dev.rtodo.r_todo_mobile.transfer.STOP"
        private const val EXTRA_TITLE = "title"
        private const val EXTRA_TEXT = "text"
        const val EXTRA_CONVERSATION = "conversation"

        /** conversation 不为空时，点通知会直接打开该设备会话（Q45）。 */
        fun start(
            context: Context,
            title: String,
            text: String,
            conversation: String? = null,
        ) {
            val intent = Intent(context, TransferForegroundService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_TITLE, title)
                putExtra(EXTRA_TEXT, text)
                if (conversation != null) putExtra(EXTRA_CONVERSATION, conversation)
            }
            try {
                ContextCompat.startForegroundService(context, intent)
            } catch (e: Exception) {
                // Android 12+ 后台限制：应用在后台且没有正在运行的前台服务时可能被拒绝。
                // 通知只是提醒，失败不应该让传输流程崩掉。
                Log.w(TAG, "启动前台服务失败", e)
            }
        }

        fun stop(context: Context) {
            try {
                context.stopService(Intent(context, TransferForegroundService::class.java))
            } catch (_: Exception) {
                // 服务未启动
            }
        }
    }

    private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                val title = intent.getStringExtra(EXTRA_TITLE) ?: "正在传输"
                val text = intent.getStringExtra(EXTRA_TEXT) ?: ""
                val conversation = intent.getStringExtra(EXTRA_CONVERSATION)
                val notification = buildNotification(title, text, conversation)
                val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
                } else {
                    0
                }
                ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
                running = true
            }
            ACTION_STOP -> stopSelf()
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        running = false
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    private fun buildNotification(
        title: String,
        text: String,
        conversation: String?,
    ) = NotificationCompat.Builder(this, CHANNEL_ID)
        .setSmallIcon(R.mipmap.ic_launcher)
        .setContentTitle(title)
        .setContentText(text)
        .setStyle(NotificationCompat.BigTextStyle().bigText(text))
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setSilent(true)
        .setPriority(NotificationCompat.PRIORITY_LOW)
        .setCategory(NotificationCompat.CATEGORY_PROGRESS)
        .setContentIntent(
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java).apply {
                    if (conversation != null) putExtra(EXTRA_CONVERSATION, conversation)
                },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            ),
        )
        .build()

    private fun createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            CHANNEL_NAME,
            NotificationManagerCompat.IMPORTANCE_LOW,
        ).apply {
            description = "文件互传进度"
            setShowBadge(false)
            enableVibration(false)
        }
        NotificationManagerCompat.from(this).createNotificationChannel(channel)
    }
}
