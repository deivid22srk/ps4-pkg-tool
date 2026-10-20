package com.deivid.pkgtool.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.Uri
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.deivid.pkgtool.MainActivity
import com.deivid.pkgtool.R
import com.deivid.pkgtool.core.NativeCore
import com.deivid.pkgtool.core.NativeJson
import com.deivid.pkgtool.core.NativeProgress
import com.deivid.pkgtool.core.PkgException
import com.deivid.pkgtool.core.localizeError
import com.deivid.pkgtool.data.HistoryEntry
import com.deivid.pkgtool.data.HistoryStore
import com.deivid.pkgtool.data.JobKind
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import java.io.File

class PkgService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var job: Job? = null
    private var history: HistoryStore? = null

    @Volatile
    private var cancelRequested = false

    @Volatile
    private var kind: JobKind? = null

    private var lastPercent = -2
    private var lastMessage = ""

    private val notificationManager by lazy {
        getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    }

    override fun onCreate() {
        super.onCreate()
        history = HistoryStore(this)
        createChannel()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_CANCEL -> {
                cancelRequested = true
                NativeCore.cancel()
                report(-1, getString(R.string.status_cancelling))
                return START_NOT_STICKY
            }

            ACTION_START -> {
                if (job?.isActive == true) return START_NOT_STICKY
                val parsedKind = when (intent.getStringExtra(EXTRA_KIND)) {
                    "pack" -> JobKind.PACK
                    "extract" -> JobKind.EXTRACT
                    else -> null
                } ?: return START_NOT_STICKY
                val input = intent.getStringExtra(EXTRA_INPUT_URI) ?: return START_NOT_STICKY
                val output = intent.getStringExtra(EXTRA_OUTPUT_URI) ?: return START_NOT_STICKY

                kind = parsedKind
                cancelRequested = false
                lastPercent = -2
                lastMessage = ""
                PkgJobState.begin(
                    parsedKind,
                    getString(
                        if (parsedKind == JobKind.EXTRACT) R.string.status_preparing_extract
                        else R.string.status_preparing_pack,
                    ),
                )
                startForegroundCompat(
                    buildProgressNotification(
                        0,
                        getString(
                            if (parsedKind == JobKind.EXTRACT) R.string.status_preparing_extract
                            else R.string.status_preparing_pack,
                        ),
                        true,
                    ),
                )

                job = scope.launch {
                    runJob(
                        kind = parsedKind,
                        inputUri = Uri.parse(input),
                        outputUri = Uri.parse(output),
                        contentId = intent.getStringExtra(EXTRA_CONTENT_ID).orEmpty(),
                        title = intent.getStringExtra(EXTRA_TITLE).orEmpty(),
                        displayName = intent.getStringExtra(EXTRA_DISPLAY_NAME).orEmpty(),
                    )
                }
            }
        }
        return START_NOT_STICKY
    }

    private suspend fun runJob(
        kind: JobKind,
        inputUri: Uri,
        outputUri: Uri,
        contentId: String,
        title: String,
        displayName: String,
    ) {
        val started = System.currentTimeMillis()
        try {
            when (kind) {
                JobKind.EXTRACT -> doExtract(inputUri, outputUri, displayName, started)
                JobKind.PACK -> doPack(inputUri, outputUri, contentId, title, displayName, started)
            }
        } catch (cancelled: CancellationException) {
            finishCancelled(started)
        } catch (error: PkgException) {
            finishError(error.code, localizeError(this, error.code, error.message), started)
        } catch (error: Throwable) {
            if (cancelRequested || error is CancellationException) {
                finishCancelled(started)
            } else {
                val code = if (error.message?.contains("space", true) == true) "NO_SPACE" else "IO"
                finishError(code, localizeError(this, code, error.message.orEmpty()), started)
            }
        }
    }

    private suspend fun doExtract(
        inputUri: Uri,
        destinationTree: Uri,
        displayName: String,
        started: Long,
    ) {
        val work = prepareWorkDir()
        try {
            val input = File(work, "input.pkg")
            report(-1, getString(R.string.status_copying_pkg))
            copyUriToFile(
                context = this,
                uri = inputUri,
                dest = input,
                isCancelled = { cancelRequested },
                progress = { pct, message -> report(mapPercent(pct, 0, 10), message) },
            )

            val extracted = File(work, "extracted")
            deleteRecursively(extracted)
            extracted.mkdirs()
            report(10, getString(R.string.status_reading_pkg))
            val json = NativeCore.extract(
                input.absolutePath,
                extracted.absolutePath,
                NativeProgress { pct, message ->
                    report(mapPercent(pct, 10, 90), message)
                },
            )
            val result = NativeJson.parseJob(json)

            report(90, getString(R.string.status_copying_result))
            val baseName = displayName.substringBeforeLast('.').ifBlank {
                result.contentId ?: "PKG"
            }
            val folderName = sanitizeName(baseName)
            val destination = copyDirectoryToTree(
                context = this,
                source = extracted,
                treeUri = destinationTree,
                folderName = folderName,
                isCancelled = { cancelRequested },
                progress = { pct, message -> report(mapPercent(pct, 90, 99), message) },
            )
            report(100, getString(R.string.status_done))
            finishSuccess(
                kind = JobKind.EXTRACT,
                message = getString(R.string.status_extract_done),
                outputUri = destination.toString(),
                label = folderName,
                files = result.fileCount,
                bytes = result.bytes,
                started = started,
            )
        } finally {
            deleteRecursively(work)
        }
    }

    private suspend fun doPack(
        sourceTree: Uri,
        outputDocument: Uri,
        contentId: String,
        title: String,
        displayName: String,
        started: Long,
    ) {
        val work = prepareWorkDir()
        try {
            val source = File(work, "source")
            report(-1, getString(R.string.status_copying_source))
            copyTreeToDirectory(
                context = this,
                treeUri = sourceTree,
                dest = source,
                isCancelled = { cancelRequested },
                progress = { pct, message -> report(mapPercent(pct, 0, 15), message) },
            )

            val output = File(work, "output.pkg")
            report(15, getString(R.string.status_building_pkg))
            val json = NativeCore.pack(
                source.absolutePath,
                output.absolutePath,
                contentId,
                title,
                NativeProgress { pct, message ->
                    report(mapPercent(pct, 15, 90), message)
                },
            )
            val result = NativeJson.parseJob(json)
            report(90, getString(R.string.status_writing_output))
            copyFileToDocument(
                context = this,
                source = output,
                outputUri = outputDocument,
                isCancelled = { cancelRequested },
                progress = { pct, message -> report(mapPercent(pct, 90, 99), message) },
            )
            report(100, getString(R.string.status_done))
            val label = displayName.ifBlank { "output.pkg" }
            finishSuccess(
                kind = JobKind.PACK,
                message = getString(R.string.status_pack_done),
                outputUri = outputDocument.toString(),
                label = label,
                files = result.fileCount,
                bytes = result.pkgSize,
                started = started,
            )
        } finally {
            deleteRecursively(work)
        }
    }

    private fun prepareWorkDir(): File {
        val work = File(cacheDir, "pkgtool/work")
        deleteRecursively(work)
        work.mkdirs()
        return work
    }

    private fun finishSuccess(
        kind: JobKind,
        message: String,
        outputUri: String?,
        label: String,
        files: Long,
        bytes: Long,
        started: Long,
    ) {
        val duration = System.currentTimeMillis() - started
        val result = JobResult(
            kind = kind,
            message = message,
            outputUri = outputUri,
            label = label,
            fileCount = files,
            bytes = bytes,
            durationMs = duration,
        )
        PkgJobState.success(result)
        history?.add(
            HistoryEntry(
                kind = kind,
                ok = true,
                label = label,
                detail = message,
                timestamp = System.currentTimeMillis(),
                durationMs = duration,
            ),
        )
        notificationManager.notify(
            NOTIFICATION_ID,
            buildFinishedNotification(success = true, message = message, outputUri = outputUri),
        )
        stopForeground(STOP_FOREGROUND_DETACH)
        stopSelf()
    }

    private fun finishError(code: String, message: String, started: Long) {
        val currentKind = kind
        val duration = System.currentTimeMillis() - started
        PkgJobState.failure(JobError(currentKind, code, message))
        history?.add(
            HistoryEntry(
                kind = currentKind ?: JobKind.EXTRACT,
                ok = false,
                label = getString(R.string.history_error_label),
                detail = message,
                timestamp = System.currentTimeMillis(),
                durationMs = duration,
            ),
        )
        notificationManager.notify(
            NOTIFICATION_ID,
            buildFinishedNotification(success = false, message = message, outputUri = null),
        )
        stopForeground(STOP_FOREGROUND_DETACH)
        stopSelf()
    }

    private fun finishCancelled(started: Long) {
        finishError("CANCELLED", getString(R.string.err_cancelled), started)
    }

    private fun report(percent: Int, message: String) {
        PkgJobState.progress(percent, message)
        synchronized(this) {
            if (percent == lastPercent && message == lastMessage) return
            lastPercent = percent
            lastMessage = message
            notificationManager.notify(
                NOTIFICATION_ID,
                buildProgressNotification(percent, message, percent < 0),
            )
        }
    }

    private fun buildProgressNotification(
        percent: Int,
        message: String,
        indeterminate: Boolean,
    ): Notification {
        val title = when (kind) {
            JobKind.PACK -> getString(R.string.notif_title_pack)
            else -> getString(R.string.notif_title_extract)
        }
        val builder = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_pkg)
            .setContentTitle(title)
            .setContentText(message)
            .setStyle(NotificationCompat.BigTextStyle().bigText(message))
            .setOnlyAlertOnce(true)
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setContentIntent(activityPendingIntent())
            .addAction(0, getString(R.string.action_cancel), cancelPendingIntent())
        if (indeterminate) {
            builder.setProgress(0, 0, true)
        } else {
            builder.setProgress(100, percent.coerceIn(0, 100), false)
        }
        return builder.build()
    }

    private fun buildFinishedNotification(
        success: Boolean,
        message: String,
        outputUri: String?,
    ): Notification {
        val builder = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_pkg)
            .setContentTitle(
                getString(if (success) R.string.notif_done else R.string.notif_failed),
            )
            .setContentText(message)
            .setStyle(NotificationCompat.BigTextStyle().bigText(message))
            .setAutoCancel(true)
            .setOngoing(false)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setContentIntent(
                if (success && outputUri != null) {
                    openResultPendingIntent(outputUri)
                } else {
                    activityPendingIntent()
                },
            )
        if (success) {
            builder.setProgress(0, 0, false)
        }
        return builder.build()
    }

    private fun activityPendingIntent(): PendingIntent {
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        return PendingIntent.getActivity(
            this,
            REQUEST_ACTIVITY,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    private fun cancelPendingIntent(): PendingIntent {
        val intent = Intent(this, PkgService::class.java).setAction(ACTION_CANCEL)
        return PendingIntent.getService(
            this,
            REQUEST_CANCEL,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    private fun openResultPendingIntent(outputUri: String): PendingIntent {
        val uri = Uri.parse(outputUri)
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/octet-stream")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        val fallback = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            putExtra(MainActivity.EXTRA_OPEN_URI, outputUri)
        }
        return PendingIntent.getActivity(
            this,
            REQUEST_OPEN,
            if (view.resolveActivity(packageManager) != null) view else fallback,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    private fun startForegroundCompat(notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                getString(R.string.notification_channel_name),
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = getString(R.string.notification_channel_description)
                setShowBadge(false)
            }
            notificationManager.createNotificationChannel(channel)
        }
    }

    override fun onDestroy() {
        cancelRequested = true
        NativeCore.cancel()
        job?.cancel()
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val ACTION_START = "com.deivid.pkgtool.action.START"
        const val ACTION_CANCEL = "com.deivid.pkgtool.action.CANCEL"

        const val EXTRA_KIND = "kind"
        const val EXTRA_INPUT_URI = "input_uri"
        const val EXTRA_OUTPUT_URI = "output_uri"
        const val EXTRA_CONTENT_ID = "content_id"
        const val EXTRA_TITLE = "title"
        const val EXTRA_DISPLAY_NAME = "display_name"

        private const val CHANNEL_ID = "pkgtool_jobs"
        private const val NOTIFICATION_ID = 42
        private const val REQUEST_ACTIVITY = 10
        private const val REQUEST_CANCEL = 11
        private const val REQUEST_OPEN = 12

        fun startExtract(
            context: Context,
            pkgUri: Uri,
            destinationTree: Uri,
            displayName: String,
        ) {
            val intent = Intent(context, PkgService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_KIND, "extract")
                putExtra(EXTRA_INPUT_URI, pkgUri.toString())
                putExtra(EXTRA_OUTPUT_URI, destinationTree.toString())
                putExtra(EXTRA_DISPLAY_NAME, displayName)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun startPack(
            context: Context,
            sourceTree: Uri,
            outputDocument: Uri,
            contentId: String,
            title: String,
            displayName: String,
        ) {
            val intent = Intent(context, PkgService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_KIND, "pack")
                putExtra(EXTRA_INPUT_URI, sourceTree.toString())
                putExtra(EXTRA_OUTPUT_URI, outputDocument.toString())
                putExtra(EXTRA_CONTENT_ID, contentId)
                putExtra(EXTRA_TITLE, title)
                putExtra(EXTRA_DISPLAY_NAME, displayName)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun cancel(context: Context) {
            val intent = Intent(context, PkgService::class.java).setAction(ACTION_CANCEL)
            context.startService(intent)
        }
    }
}

private fun mapPercent(value: Int, start: Int, end: Int): Int {
    if (value < 0) return -1
    return start + ((end - start) * value) / 100
}
