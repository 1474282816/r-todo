package dev.rtodo.r_todo_mobile

import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
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
                    else -> result.notImplemented()
                }
            }
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
