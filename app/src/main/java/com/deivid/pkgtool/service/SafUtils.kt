package com.deivid.pkgtool.service

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.MimeTypeMap
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.util.Locale
import kotlin.coroutines.cancellation.CancellationException

typealias CopyProgress = (percent: Int, message: String) -> Unit

data class TreeMeasure(val bytes: Long, val files: Int)

fun persistTreePermission(context: Context, uri: Uri) {
    runCatching {
        context.contentResolver.takePersistableUriPermission(
            uri,
            Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
        )
    }
}

fun persistDocumentPermission(context: Context, uri: Uri, write: Boolean = false) {
    val flags = if (write) {
        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
    } else {
        Intent.FLAG_GRANT_READ_URI_PERMISSION
    }
    runCatching {
        context.contentResolver.takePersistableUriPermission(uri, flags)
    }
}

fun documentName(context: Context, uri: Uri): String {
    return DocumentFile.fromSingleUri(context, uri)?.name
        ?: DocumentFile.fromTreeUri(context, uri)?.name
        ?: uri.lastPathSegment
        ?: "documento"
}

fun copyUriToFile(
    context: Context,
    uri: Uri,
    dest: File,
    isCancelled: () -> Boolean,
    progress: CopyProgress,
) {
    dest.parentFile?.mkdirs()
    val total = runCatching {
        context.contentResolver.openAssetFileDescriptor(uri, "r")?.use { it.length }
    }.getOrNull()?.takeIf { it > 0 } ?: -1L

    val input = context.contentResolver.openInputStream(uri)
        ?: throw IOException("Não foi possível abrir o arquivo de origem")
    val started = System.nanoTime()
    input.use { source ->
        FileOutputStream(dest).use { output ->
            val buffer = ByteArray(1024 * 1024)
            var done = 0L
            while (true) {
                if (isCancelled()) throw CancellationException("Cancelado")
                val read = source.read(buffer)
                if (read <= 0) break
                output.write(buffer, 0, read)
                done += read
                val pct = if (total > 0) ((done * 100) / total).toInt() else -1
                val label = if (total > 0) {
                    transferMessage("Copiando arquivo", done, total, started)
                } else {
                    "Copiando arquivo…"
                }
                progress(pct.coerceIn(-1, 99), label)
            }
            output.fd.sync()
        }
    }
}

fun measureDocumentTree(document: DocumentFile): TreeMeasure {
    var bytes = 0L
    var files = 0
    fun walk(node: DocumentFile) {
        if (node.isDirectory) {
            node.listFiles().forEach { walk(it) }
        } else if (node.isFile) {
            files++
            bytes += node.length().coerceAtLeast(0L)
        }
    }
    walk(document)
    return TreeMeasure(bytes, files)
}

fun measureFileTree(root: File): TreeMeasure {
    var bytes = 0L
    var files = 0
    fun walk(file: File) {
        if (file.isDirectory) {
            file.listFiles()?.forEach { walk(it) }
        } else if (file.isFile) {
            files++
            bytes += file.length()
        }
    }
    walk(root)
    return TreeMeasure(bytes, files)
}

fun copyTreeToDirectory(
    context: Context,
    treeUri: Uri,
    dest: File,
    isCancelled: () -> Boolean,
    progress: CopyProgress,
) {
    val root = DocumentFile.fromTreeUri(context, treeUri)
        ?: throw IOException("Pasta de origem indisponível")
    dest.mkdirs()
    val total = measureDocumentTree(root)
    val started = System.nanoTime()
    var doneBytes = 0L
    var doneFiles = 0

    fun walk(node: DocumentFile, target: File, relative: String) {
        if (isCancelled()) throw CancellationException("Cancelado")
        if (node.isDirectory) {
            target.mkdirs()
            node.listFiles()
                .sortedBy { it.name.orEmpty().lowercase() }
                .forEach { child ->
                    val name = sanitizeName(child.name ?: "arquivo")
                    walk(child, File(target, name), if (relative.isEmpty()) name else "$relative/$name")
                }
        } else if (node.isFile) {
            target.parentFile?.mkdirs()
            val input = context.contentResolver.openInputStream(node.uri)
                ?: throw IOException("Não foi possível ler ${node.name}")
            input.use { source ->
                FileOutputStream(target).use { output ->
                    val buffer = ByteArray(1024 * 1024)
                    while (true) {
                        if (isCancelled()) throw CancellationException("Cancelado")
                        val read = source.read(buffer)
                        if (read <= 0) break
                        output.write(buffer, 0, read)
                        doneBytes += read
                        reportCopy(progress, doneBytes, doneFiles, total, relative, started)
                    }
                }
            }
            doneFiles++
            reportCopy(progress, doneBytes, doneFiles, total, relative, started)
        }
    }

    walk(root, dest, "")
}

fun copyDirectoryToTree(
    context: Context,
    source: File,
    treeUri: Uri,
    folderName: String,
    isCancelled: () -> Boolean,
    progress: CopyProgress,
): Uri {
    val root = DocumentFile.fromTreeUri(context, treeUri)
        ?: throw IOException("Pasta de destino indisponível")
    val name = uniqueChildName(root, sanitizeName(folderName))
    val destination = root.createDirectory(name)
        ?: throw IOException("Não foi possível criar a pasta de destino")
    val total = measureFileTree(source)
    val started = System.nanoTime()
    var doneBytes = 0L
    var doneFiles = 0

    try {
        fun walk(dir: File, target: DocumentFile, relative: String) {
            if (isCancelled()) throw CancellationException("Cancelado")
            val children = dir.listFiles()?.sortedBy { it.name.lowercase() } ?: emptyList()
            for (child in children) {
                val childName = sanitizeName(child.name)
                val rel = if (relative.isEmpty()) childName else "$relative/$childName"
                if (child.isDirectory) {
                    val sub = target.createDirectory(childName)
                        ?: throw IOException("Não foi possível criar a pasta $childName")
                    walk(child, sub, rel)
                } else if (child.isFile) {
                    val doc = target.createFile(mimeFor(childName), childName)
                        ?: throw IOException("Não foi possível criar o arquivo $childName")
                    val output = context.contentResolver.openOutputStream(doc.uri, "wt")
                        ?: throw IOException("Não foi possível gravar $childName")
                    FileInputStream(child).use { input ->
                        output.use { out ->
                            val buffer = ByteArray(1024 * 1024)
                            while (true) {
                                if (isCancelled()) throw CancellationException("Cancelado")
                                val read = input.read(buffer)
                                if (read <= 0) break
                                out.write(buffer, 0, read)
                                doneBytes += read
                                reportCopy(progress, doneBytes, doneFiles, total, rel, started)
                            }
                        }
                    }
                    doneFiles++
                    reportCopy(progress, doneBytes, doneFiles, total, rel, started)
                }
            }
        }
        walk(source, destination, "")
    } catch (t: Throwable) {
        deleteDocumentTree(destination)
        throw t
    }
    return destination.uri
}

fun copyFileToDocument(
    context: Context,
    source: File,
    outputUri: Uri,
    isCancelled: () -> Boolean,
    progress: CopyProgress,
) {
    try {
        val output = context.contentResolver.openOutputStream(outputUri, "wt")
            ?: throw IOException("Não foi possível abrir o destino")
        val started = System.nanoTime()
        output.use { out ->
            FileInputStream(source).use { sourceStream ->
                val total = source.length().coerceAtLeast(1L)
                var done = 0L
                val buffer = ByteArray(1024 * 1024)
                while (true) {
                    if (isCancelled()) throw CancellationException("Cancelado")
                    val read = sourceStream.read(buffer)
                    if (read <= 0) break
                    out.write(buffer, 0, read)
                    done += read
                    val pct = ((done * 100) / total).toInt().coerceIn(0, 99)
                    progress(pct, transferMessage("Gravando PKG de saída", done, total, started))
                }
                out.flush()
            }
        }
    } catch (t: Throwable) {
        // Não deixe um PKG truncado no destino quando a operação falhar.
        runCatching {
            DocumentFile.fromSingleUri(context, outputUri)?.delete()
        }
        throw t
    }
}

private fun deleteDocumentTree(document: DocumentFile) {
    runCatching {
        if (document.isDirectory) {
            document.listFiles().forEach { deleteDocumentTree(it) }
        }
        document.delete()
    }
}

fun uniqueChildName(parent: DocumentFile, base: String): String {
    val existing = parent.listFiles().mapNotNull { it.name }.toSet()
    if (base !in existing) return base
    var index = 2
    while (true) {
        val candidate = "$base ($index)"
        if (candidate !in existing) return candidate
        index++
    }
}

fun sanitizeName(name: String): String {
    val clean = name
        .replace('/', '_')
        .replace('\\', '_')
        .replace(':', '_')
        .replace('*', '_')
        .replace('?', '_')
        .replace('"', '_')
        .replace('<', '_')
        .replace('>', '_')
        .replace('|', '_')
        .trim()
        .trimEnd('.')
    return clean.ifEmpty { "arquivo" }
}

fun mimeFor(name: String): String {
    val extension = name.substringAfterLast('.', "").lowercase()
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)
        ?: "application/octet-stream"
}

fun deleteRecursively(file: File) {
    runCatching {
        if (file.isDirectory) {
            file.listFiles()?.forEach { deleteRecursively(it) }
        }
        file.delete()
    }
}

private fun reportCopy(
    progress: CopyProgress,
    doneBytes: Long,
    doneFiles: Int,
    total: TreeMeasure,
    relative: String,
    startedNanos: Long,
) {
    val pct = when {
        total.bytes > 0 -> ((doneBytes * 100) / total.bytes).toInt()
        total.files > 0 -> ((doneFiles * 100) / total.files).toInt()
        else -> -1
    }
    val base = relative.ifEmpty { "Copiando…" }
    val label = if (total.bytes > 0) {
        transferMessage(base, doneBytes, total.bytes, startedNanos)
    } else {
        base
    }
    progress(pct.coerceIn(-1, 99), label)
}

private fun transferMessage(prefix: String, done: Long, total: Long, startedNanos: Long): String {
    val elapsed = ((System.nanoTime() - startedNanos).coerceAtLeast(1L)) / 1_000_000_000.0
    val speed = (done.toDouble() / elapsed).coerceAtLeast(0.0)
    val remaining = (total - done).coerceAtLeast(0L)
    val eta = if (speed > 1.0) remaining / speed else 0.0
    return "$prefix • ${formatSpeed(speed)}/s • ETA ${formatEta(eta)}"
}

private fun formatSpeed(bytesPerSecond: Double): String = when {
    bytesPerSecond >= 1_000_000_000.0 -> String.format(Locale.US, "%.1f GB", bytesPerSecond / 1_000_000_000.0)
    bytesPerSecond >= 1_000_000.0 -> String.format(Locale.US, "%.1f MB", bytesPerSecond / 1_000_000.0)
    bytesPerSecond >= 1_000.0 -> String.format(Locale.US, "%.1f KB", bytesPerSecond / 1_000.0)
    else -> String.format(Locale.US, "%.0f B", bytesPerSecond)
}

private fun formatEta(seconds: Double): String {
    if (!seconds.isFinite() || seconds <= 0.0) return "—"
    val total = kotlin.math.ceil(seconds).toLong()
    return when {
        total >= 3600 -> "${total / 3600}h${(total % 3600) / 60}m"
        total >= 60 -> "${total / 60}m${total % 60}s"
        else -> "${total}s"
    }
}
