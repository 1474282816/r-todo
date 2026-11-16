package dev.rtodo.r_todo_mobile

import android.app.DownloadManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import androidx.core.content.FileProvider
import dev.rtodo.r_todo_mobile.transfer.TransferForegroundService
import dev.rtodo.r_todo_mobile.transfer.TransferPlugin
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {

    companion object {
        /** 点击常驻通知带过来的会话 fingerprint，Flutter 初始化后取走。 */
        @Volatile
        var pendingConversation: String? = null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        consumeConversation(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        consumeConversation(intent)
    }

    private fun consumeConversation(intent: Intent?) {
        val fingerprint =
            intent?.getStringExtra(TransferForegroundService.EXTRA_CONVERSATION)
        if (!fingerprint.isNullOrEmpty()) pendingConversation = fingerprint
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        // 局域网互传的平台能力（SAF / MediaStore / 前台服务 / 组播锁）
        flutterEngine.plugins.add(TransferPlugin())

        // 安装器（原有能力，保持不变）
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "r_todo/install")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "installApk" -> {
                        val path = call.arguments as? String
                        if (path.isNullOrEmpty()) {
                            result.error("bad_path", "path 为空", null)
                        } else {
                            try {
                                installApk(path)
                                result.success(true)
                            } catch (e: Exception) {
                                result.error("install_failed", e.message, null)
                            }
                        }
                    }
                    "openInstallSettings" -> {
                        openInstallSettings()
                        result.success(true)
                    }
                    "showReminder" -> {
                        val title = call.argument<String>("title") ?: "rtools"
                        val text = call.argument<String>("text") ?: "任务提醒"
                        TransferForegroundService.start(this, title, text)
                        result.success(true)
                    }
                    else -> result.notImplemented()
                }
            }

        // 会话式互传：通知详情 / 点击直达 / 长按菜单能力（Q25/Q45）。
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "rtodo/chat")
            .setMethodCallHandler { call, result ->
                try {
                    when (call.method) {
                        "showNotification" -> {
                            TransferForegroundService.start(
                                this,
                                call.argument<String>("title") ?: "rtools",
                                call.argument<String>("text") ?: "",
                                call.argument<String>("conversation"),
                            )
                            result.success(null)
                        }
                        "pendingConversation" -> {
                            val fingerprint = pendingConversation
                            pendingConversation = null
                            result.success(fingerprint)
                        }
                        "deleteFile" -> {
                            val uri = Uri.parse(call.argument<String>("uri") ?: "")
                            result.success(deleteUri(uri))
                        }
                        "copyImage" -> {
                            val uri = Uri.parse(call.argument<String>("uri") ?: "")
                            val clipboard =
                                getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            clipboard.setPrimaryClip(
                                ClipData.newUri(contentResolver, "图片", uri)
                            )
                            result.success(null)
                        }
                        "showInFiles" -> {
                            startActivity(
                                Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)
                                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            )
                            result.success(null)
                        }
                        "saveToGallery" -> {
                            val uri = Uri.parse(call.argument<String>("uri") ?: "")
                            result.success(
                                saveToGallery(uri, call.argument<String>("name"))
                            )
                        }
                        "openFile" -> {
                            openFile(
                                Uri.parse(call.argument<String>("uri") ?: "")
                            )
                            result.success(null)
                        }
                        else -> result.notImplemented()
                    }
                } catch (e: Exception) {
                    result.error("chat_failed", e.message, null)
                }
            }
    }

    /** 只删 MediaStore 里的接收副本；发送方原始文件不在这个 URI 体系里。 */
    private fun deleteUri(uri: Uri): Int {
        if (uri.scheme == "content") return contentResolver.delete(uri, null, null)
        val path = uri.path ?: return 0
        return if (File(path).delete()) 1 else 0
    }

    private fun openFile(uri: Uri) {
        val mime = contentResolver.getType(uri) ?: "*/*"
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, mime)
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK
            )
        }
        startActivity(
            Intent.createChooser(view, "打开")
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }

    private fun saveToGallery(uri: Uri, name: String?): String? {
        val resolver = contentResolver
        val values = ContentValues().apply {
            put(
                MediaStore.Images.Media.DISPLAY_NAME,
                name ?: "rtools-${System.currentTimeMillis()}.jpg",
            )
            put(
                MediaStore.Images.Media.MIME_TYPE,
                resolver.getType(uri) ?: "image/jpeg",
            )
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/rtools")
                put(MediaStore.Images.Media.IS_PENDING, 1)
            }
        }
        val target = resolver.insert(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            values,
        ) ?: return null
        resolver.openInputStream(uri)?.use { input ->
            resolver.openOutputStream(target)?.use { output -> input.copyTo(output) }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            values.clear()
            values.put(MediaStore.Images.Media.IS_PENDING, 0)
            resolver.update(target, values, null, null)
        }
        return target.toString()
    }

    /// 通过 FileProvider 拉起系统安装器安装 APK
    private fun installApk(path: String) {
        val file = File(path)
        val uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", file)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startActivity(intent)
    }

    /// 打开"安装未知应用"设置页
    private fun openInstallSettings() {
        val intent = Intent(
            android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
            Uri.parse("package:$packageName")
        )
        startActivity(intent)
    }
}
