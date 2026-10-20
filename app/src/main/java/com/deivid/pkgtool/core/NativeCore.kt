package com.deivid.pkgtool.core

import android.content.Context
import androidx.annotation.Keep
import com.deivid.pkgtool.R
import org.json.JSONObject

/**
 * Ponte JNI com o núcleo Rust (`pkg_core`).
 *
 * O núcleo recebe caminhos absolutos (arquivos temporários do app) ou um
 * file descriptor do Storage Access Framework. Ele devolve JSON com `ok`,
 * `message`/`code`.
 */
object NativeCore {
    init {
        System.loadLibrary("pkg_core")
    }

    external fun analyze(path: String): String
    external fun analyzeFd(fd: Int): String
    external fun extract(pkgPath: String, outputDir: String, progress: NativeProgress): String
    external fun pack(
        inputDir: String,
        outputPath: String,
        contentId: String,
        title: String,
        progress: NativeProgress,
    ): String

    external fun cancel()
    external fun version(): String
}

/**
 * Objeto de callback chamado pelo Rust na mesma thread da operação.
 * O método precisa manter exatamente esta assinatura.
 */
class NativeProgress(private val onUpdate: (percent: Int, message: String) -> Unit) {
    @Keep
    fun onProgress(percent: Int, message: String) {
        onUpdate(percent, message)
    }
}

class PkgException(
    val code: String,
    override val message: String,
) : Exception(message)

data class AnalyzeInfo(
    val contentId: String,
    val title: String,
    val titleId: String,
    val category: String,
    val contentType: String,
    val pkgSize: Long,
    val pfsSize: Long,
    val entryCount: Int,
    val fileCount: Long,
    val totalBytes: Long,
    val encrypted: Boolean,
    val warning: String,
)

data class NativeJobResult(
    val message: String,
    val contentId: String?,
    val title: String?,
    val titleId: String?,
    val fileCount: Long,
    val bytes: Long,
    val pkgSize: Long,
)

object NativeJson {
    fun parseAnalyze(json: String): AnalyzeInfo {
        val obj = checkOk(json)
        return AnalyzeInfo(
            contentId = obj.optString("content_id"),
            title = obj.optString("title"),
            titleId = obj.optString("title_id"),
            category = obj.optString("category"),
            contentType = obj.optString("content_type"),
            pkgSize = obj.optLong("pkg_size"),
            pfsSize = obj.optLong("pfs_size"),
            entryCount = obj.optInt("entry_count"),
            fileCount = obj.optLong("file_count"),
            totalBytes = obj.optLong("total_bytes"),
            encrypted = obj.optBoolean("encrypted"),
            warning = obj.optString("warning"),
        )
    }

    fun parseJob(json: String): NativeJobResult {
        val obj = checkOk(json)
        return NativeJobResult(
            message = obj.optString("message"),
            contentId = obj.optString("contentId").ifEmpty { null },
            title = obj.optString("title").ifEmpty { null },
            titleId = obj.optString("titleId").ifEmpty { null },
            fileCount = obj.optLong("fileCount"),
            bytes = obj.optLong("bytes"),
            pkgSize = obj.optLong("pkgSize"),
        )
    }

    private fun checkOk(json: String): JSONObject {
        val obj = runCatching { JSONObject(json) }.getOrElse {
            throw PkgException("INTERNAL", "Resposta inválida do núcleo: ${it.message}")
        }
        if (!obj.optBoolean("ok", false)) {
            throw PkgException(
                obj.optString("code", "INTERNAL"),
                obj.optString("message", "Falha desconhecida no núcleo"),
            )
        }
        return obj
    }
}

fun localizeError(context: Context, code: String, fallback: String): String {
    val res = when (code) {
        "CANCELLED" -> R.string.err_cancelled
        "INVALID_PKG" -> R.string.err_invalid_pkg
        "ENCRYPTED_PKG" -> R.string.err_encrypted
        "NO_PFS" -> R.string.err_no_pfs
        "NO_SPACE" -> R.string.err_no_space
        "INVALID_INPUT" -> R.string.err_invalid_input
        "UNSUPPORTED" -> R.string.err_unsupported
        "IO" -> R.string.err_io
        else -> null
    }
    return if (res != null) {
        context.getString(res)
    } else {
        fallback.ifBlank { context.getString(R.string.err_unknown) }
    }
}
