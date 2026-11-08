package dev.rtodo.r_todo_mobile.transfer

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.TimeUnit

/**
 * SAF 选择 / cache 暂存 / MediaStore 落盘。
 *
 * 约定：
 * - 发送：content:// → cacheDir/transfer/outbox/<uuid>-<name>，把**普通文件路径**交给 Rust
 * - 接收：Rust 写 cacheDir/transfer/inbox/<fileId>，本类再复制到 Downloads/r-todo
 */
class FileBridge(private val context: Context) {

    companion object {
        private const val TAG = "RtodoFileBridge"
        const val RECEIVE_DIR_NAME = "r-todo"
        private const val MAX_FILE_NAME = 120
        private val STALE_MS = TimeUnit.HOURS.toMillis(24)
    }

    fun transferDir(): File = File(context.cacheDir, "transfer").apply { mkdirs() }

    fun outboxDir(): File = File(transferDir(), "outbox").apply { mkdirs() }

    fun inboxDir(): File = File(transferDir(), "inbox").apply { mkdirs() }

    /** 复制选中的 URI 到 cache，返回 `{path, name, size, mime}` 列表。 */
    fun stageFromUris(uris: List<Uri>): List<Map<String, Any?>> {
        val outDir = outboxDir()
        return uris.mapNotNull { uri ->
            try {
                val resolver = context.contentResolver
                val displayName = queryDisplayName(uri)
                    ?: "file-${System.currentTimeMillis()}"
                val mime = resolver.getType(uri)
                val safeName = sanitizeFileName(displayName)

                val target = File(outDir, "${UUID.randomUUID()}-$safeName")
                resolver.openInputStream(uri)?.use { input ->
                    target.outputStream().use { output -> input.copyTo(output) }
                } ?: return@mapNotNull null

                mapOf(
                    "path" to target.absolutePath,
                    "name" to displayName,
                    "size" to target.length(),
                    "mime" to mime,
                )
            } catch (e: Exception) {
                Log.w(TAG, "复制文件失败: $uri", e)
                null
            }
        }
    }

    /** 写入系统下载目录 `Downloads/r-todo`，返回 `{uri, displayName}`。 */
    fun saveToDownloads(tempPath: String, fileName: String, mime: String?): Map<String, String> {
        val source = File(tempPath)
        if (!source.exists()) throw IOException("临时文件不存在: $tempPath")

        val safeName = sanitizeFileName(fileName)
        val resolvedMime = mime ?: "application/octet-stream"

        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            saveViaMediaStore(source, safeName, resolvedMime)
        } else {
            saveLegacy(source, safeName)
        }
    }

    private fun saveViaMediaStore(
        source: File,
        fileName: String,
        mime: String,
    ): Map<String, String> {
        val resolver = context.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, fileName)
            put(MediaStore.Downloads.MIME_TYPE, mime)
            put(
                MediaStore.Downloads.RELATIVE_PATH,
                "${Environment.DIRECTORY_DOWNLOADS}/$RECEIVE_DIR_NAME",
            )
            put(MediaStore.Downloads.IS_PENDING, 1)
        }

        val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        val uri = resolver.insert(collection, values)
            ?: throw IOException("无法创建下载项")

        try {
            resolver.openOutputStream(uri)?.use { output ->
                source.inputStream().use { input -> input.copyTo(output) }
            } ?: throw IOException("无法写入下载项")

            val done = ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }
            resolver.update(uri, done, null, null)
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }

        val displayName = queryDisplayName(uri) ?: fileName
        return mapOf("uri" to uri.toString(), "displayName" to displayName)
    }

    @Suppress("DEPRECATION")
    private fun saveLegacy(source: File, fileName: String): Map<String, String> {
        val dir = File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
            RECEIVE_DIR_NAME,
        )
        if (!dir.exists() && !dir.mkdirs()) {
            throw IOException("无法创建目录: ${dir.absolutePath}")
        }

        val target = uniqueFile(dir, fileName)
        source.copyTo(target, overwrite = false)
        return mapOf("uri" to Uri.fromFile(target).toString(), "displayName" to target.name)
    }

    /** 删除指定文件（失败静默）。 */
    fun cleanup(paths: List<String>) {
        for (path in paths) {
            try {
                File(path).delete()
            } catch (e: Exception) {
                Log.w(TAG, "清理失败: $path", e)
            }
        }
    }

    /** 启动时清理 24h 前的残留（异常退出/被杀时留下的东西）。 */
    fun cleanupStale() {
        val deadline = System.currentTimeMillis() - STALE_MS
        listOf(outboxDir(), inboxDir()).forEach { dir ->
            dir.listFiles()?.forEach { file ->
                if (file.lastModified() < deadline) file.delete()
            }
        }
    }

    private fun queryDisplayName(uri: Uri): String? {
        return try {
            context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
            }
        } catch (e: Exception) {
            Log.w(TAG, "查询文件名失败: $uri", e)
            null
        }
    }

    /** 文件名清洗：去掉路径分隔符与控制字符，限制长度，避免覆盖已有文件。 */
    fun sanitizeFileName(raw: String): String {
        val cleaned = raw
            .replace(Regex("[/\\\\:*?\"<>|\\u0000-\\u001f]"), "_")
            .trim()
            .trim('.')
            .ifEmpty { "file-${System.currentTimeMillis()}" }

        if (cleaned.length <= MAX_FILE_NAME) return cleaned

        val dot = cleaned.lastIndexOf('.')
        return if (dot > 0 && cleaned.length - dot <= 12) {
            cleaned.take(MAX_FILE_NAME - (cleaned.length - dot)) + cleaned.substring(dot)
        } else {
            cleaned.take(MAX_FILE_NAME)
        }
    }

    private fun uniqueFile(dir: File, fileName: String): File {
        val base = fileName.substringBeforeLast('.', fileName)
        val ext = fileName.substringAfterLast('.', "")
        var candidate = File(dir, fileName)
        var index = 1
        while (candidate.exists()) {
            val name = if (ext.isEmpty()) "$base ($index)" else "$base ($index).$ext"
            candidate = File(dir, name)
            index += 1
        }
        return candidate
    }
}
