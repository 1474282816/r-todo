package dev.rtodo.r_todo_mobile.transfer

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.PluginRegistry

/**
 * 互传的平台能力：SAF 选择、MediaStore 落盘、前台服务、组播锁、通知权限、cache 清理。
 * 传输协议本身在 Rust（FRB），本类不碰网络。
 */
class TransferPlugin :
    FlutterPlugin,
    MethodChannel.MethodCallHandler,
    ActivityAware,
    PluginRegistry.ActivityResultListener {

    companion object {
        private const val TAG = "RtodoTransferPlugin"
        const val CHANNEL = "rtodo/transfer"
        private const val REQUEST_PICK_FILES = 6417
        private const val REQUEST_NOTIFICATIONS = 6418
        private const val REQUEST_STORAGE = 6419
    }

    private var channel: MethodChannel? = null
    private var appContext: Context? = null
    private var activity: Activity? = null
    private var bridge: FileBridge? = null
    private var pendingPick: MethodChannel.Result? = null
    private var multicastLock: WifiManager.MulticastLock? = null

    private val mainHandler = Handler(Looper.getMainLooper())

    // ---- FlutterPlugin ----

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        appContext = binding.applicationContext
        bridge = FileBridge(binding.applicationContext)
        channel = MethodChannel(binding.binaryMessenger, CHANNEL).also {
            it.setMethodCallHandler(this)
        }
        // 上次异常退出/被杀留下的 cache
        Thread {
            try {
                bridge?.cleanupStale()
            } catch (e: Exception) {
                Log.w(TAG, "清理残留失败", e)
            }
        }.start()
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        channel?.setMethodCallHandler(null)
        channel = null
        releaseMulticastLock()
        appContext = null
        bridge = null
    }

    // ---- ActivityAware ----

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        activity = binding.activity
        binding.addActivityResultListener(this)
    }

    override fun onDetachedFromActivityForConfigChanges() = detachActivity()

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) =
        onAttachedToActivity(binding)

    override fun onDetachedFromActivity() = detachActivity()

    private fun detachActivity() {
        activity = null
        pendingPick?.error("detached", "界面已关闭", null)
        pendingPick = null
    }

    // ---- MethodChannel ----

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "pickFiles" -> pickFiles(call, result)
            "saveToDownloads" -> saveToDownloads(call, result)
            "acquireMulticastLock" -> {
                acquireMulticastLock()
                result.success(null)
            }
            "releaseMulticastLock" -> {
                releaseMulticastLock()
                result.success(null)
            }
            "startForeground" -> {
                val context = appContext
                if (context == null) {
                    result.error("no_context", "应用上下文不可用", null)
                } else {
                    TransferForegroundService.start(
                        context,
                        call.argument<String>("title") ?: "正在传输",
                        call.argument<String>("text") ?: "",
                    )
                    result.success(null)
                }
            }
            "stopForeground" -> {
                appContext?.let { TransferForegroundService.stop(it) }
                result.success(null)
            }
            "requestNotificationPermission" -> result.success(requestNotificationPermission())
            "cleanupCache" -> {
                val paths = call.argument<List<String>>("paths") ?: emptyList()
                bridge?.cleanup(paths)
                result.success(null)
            }
            else -> result.notImplemented()
        }
    }

    // ---- 文件选择 ----

    private fun pickFiles(call: MethodCall, result: MethodChannel.Result) {
        val host = activity
        if (host == null) {
            result.error("no_activity", "当前没有可用界面", null)
            return
        }
        if (pendingPick != null) {
            result.error("busy", "上一次选择还没结束", null)
            return
        }

        pendingPick = result
        val multiple = call.argument<Boolean>("multiple") ?: true
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple)
        }

        try {
            @Suppress("DEPRECATION")
            host.startActivityForResult(intent, REQUEST_PICK_FILES)
        } catch (e: Exception) {
            pendingPick = null
            result.error("no_picker", e.message ?: "无法打开文件选择器", null)
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_PICK_FILES) return false

        val pending = pendingPick ?: return true
        pendingPick = null

        if (resultCode != Activity.RESULT_OK || data == null) {
            mainHandler.post { pending.success(emptyList<Map<String, Any?>>()) }
            return true
        }

        val uris = mutableListOf<Uri>()
        data.clipData?.let { clip ->
            for (index in 0 until clip.itemCount) {
                clip.getItemAt(index)?.uri?.let { uris.add(it) }
            }
        } ?: data.data?.let { uris.add(it) }

        val fileBridge = bridge
        if (fileBridge == null) {
            mainHandler.post { pending.error("no_context", "应用上下文不可用", null) }
            return true
        }

        Thread {
            val staged = try {
                fileBridge.stageFromUris(uris)
            } catch (e: Exception) {
                Log.w(TAG, "暂存文件失败", e)
                emptyList()
            }
            mainHandler.post { pending.success(staged) }
        }.start()

        return true
    }

    // ---- 落盘 ----

    private fun saveToDownloads(call: MethodCall, result: MethodChannel.Result) {
        val tempPath = call.argument<String>("tempPath")
        val fileName = call.argument<String>("fileName")
        val mime = call.argument<String>("mime")
        val fileBridge = bridge

        if (tempPath.isNullOrEmpty() || fileName.isNullOrEmpty() || fileBridge == null) {
            result.error("bad_args", "tempPath/fileName 为空", null)
            return
        }

        // Android 6~9 写公共 Downloads 需要运行时存储权限（10+ 走 MediaStore 不需要）
        if (Build.VERSION.SDK_INT in Build.VERSION_CODES.M..Build.VERSION_CODES.P &&
            !hasStoragePermission()
        ) {
            activity?.let {
                ActivityCompat.requestPermissions(
                    it,
                    arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE),
                    REQUEST_STORAGE,
                )
            }
            result.error("permission_denied", "请允许存储权限后重试", null)
            return
        }

        Thread {
            try {
                val saved = fileBridge.saveToDownloads(tempPath, fileName, mime)
                mainHandler.post { result.success(saved) }
            } catch (e: SecurityException) {
                Log.w(TAG, "写入下载目录被拒绝", e)
                mainHandler.post {
                    result.error("permission_denied", "没有存储权限，无法保存到下载目录", null)
                }
            } catch (e: Exception) {
                Log.w(TAG, "写入下载目录失败", e)
                mainHandler.post { result.error("save_failed", e.message ?: "保存失败", null) }
            }
        }.start()
    }

    // ---- 组播锁 ----

    private fun acquireMulticastLock() {
        if (multicastLock?.isHeld == true) return
        val context = appContext ?: return
        try {
            val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
                ?: return
            multicastLock = wifi.createMulticastLock("rtodo-transfer").apply {
                setReferenceCounted(false)
                acquire()
            }
        } catch (e: Exception) {
            Log.w(TAG, "获取组播锁失败", e)
        }
    }

    private fun releaseMulticastLock() {
        try {
            if (multicastLock?.isHeld == true) multicastLock?.release()
        } catch (e: Exception) {
            Log.w(TAG, "释放组播锁失败", e)
        } finally {
            multicastLock = null
        }
    }

    private fun hasStoragePermission(): Boolean {
        val context = appContext ?: return false
        return ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.WRITE_EXTERNAL_STORAGE,
        ) == PackageManager.PERMISSION_GRANTED
    }

    // ---- 通知权限 ----

    private fun requestNotificationPermission(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return true

        val context = appContext ?: return false
        val granted = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.POST_NOTIFICATIONS,
        ) == PackageManager.PERMISSION_GRANTED

        if (!granted) {
            activity?.let {
                ActivityCompat.requestPermissions(
                    it,
                    arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                    REQUEST_NOTIFICATIONS,
                )
            }
        }
        // 结果异步返回；这里只告诉 Dart 当前状态，授权后再开一次互传即可。
        return granted
    }
}
