package com.deivid.pkgtool.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.text.format.Formatter
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.Cancel
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.InsertDriveFile
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.Memory
import androidx.compose.material.icons.rounded.Save
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LargeTopAppBar
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.math.PI
import kotlin.math.sin
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.deivid.pkgtool.BuildConfig
import com.deivid.pkgtool.PkgViewModel
import com.deivid.pkgtool.R
import com.deivid.pkgtool.core.localizeError
import com.deivid.pkgtool.data.HistoryEntry
import com.deivid.pkgtool.data.JobKind
import com.deivid.pkgtool.service.JobError
import com.deivid.pkgtool.service.JobResult
import com.deivid.pkgtool.service.JobUiState
import com.deivid.pkgtool.service.PkgJobState
import java.text.DateFormat
import java.util.Date

@Composable
fun PkgToolApp(viewModel: PkgViewModel) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val job by PkgJobState.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val snackbar = remember { SnackbarHostState() }
    var showAbout by rememberSaveable { mutableStateOf(false) }

    val pkgPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri -> uri?.let(viewModel::onPkgPicked) }

    val extractDestPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri -> uri?.let(viewModel::onExtractDestinationPicked) }

    val packSourcePicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri -> uri?.let(viewModel::onPackSourcePicked) }

    val packOutputPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri -> uri?.let(viewModel::onPackOutputPicked) }

    val notificationPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { }

    LaunchedEffect(Unit) {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.POST_NOTIFICATIONS,
            ) != PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    LaunchedEffect(state.snackbar) {
        state.snackbar?.let {
            snackbar.showSnackbar(it)
            viewModel.clearSnackbar()
        }
    }

    LaunchedEffect(job.result, job.error) {
        if (job.result != null || job.error != null) {
            viewModel.refreshHistory()
        }
    }

    if (showAbout) {
        AboutDialog(onDismiss = { showAbout = false })
    }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        containerColor = MaterialTheme.colorScheme.background,
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            LargeTopAppBar(
                title = {
                    Text(
                        text = stringResource(R.string.app_name),
                        fontWeight = FontWeight.Bold,
                    )
                },
                actions = {
                    IconButton(onClick = { showAbout = true }) {
                        Icon(
                            imageVector = Icons.Rounded.Info,
                            contentDescription = stringResource(R.string.about_title),
                        )
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item(key = "hero") {
                Hero()
            }
            item(key = "job") {
                if (job.running || job.result != null || job.error != null) {
                    JobCard(
                        job = job,
                        onCancel = viewModel::cancelJob,
                        onDismiss = { PkgJobState.dismiss() },
                        onOpen = { result -> openResult(context, result) },
                        onError = { message -> viewModel.showSnackbar(message) },
                    )
                }
            }
            item(key = "actions") {
                BoxWithConstraints {
                    val wide = maxWidth >= 720.dp
                    val extract: @Composable (Modifier) -> Unit = { modifier ->
                        ExtractCard(
                            state = state,
                            running = job.running,
                            modifier = modifier,
                            onPickPkg = { pkgPicker.launch(arrayOf("*/*")) },
                            onPickDestination = { extractDestPicker.launch(null) },
                            onStart = viewModel::startExtract,
                        )
                    }
                    val pack: @Composable (Modifier) -> Unit = { modifier ->
                        PackCard(
                            state = state,
                            running = job.running,
                            modifier = modifier,
                            onPickSource = { packSourcePicker.launch(null) },
                            onPickOutput = { packOutputPicker.launch("output.pkg") },
                            onContentIdChange = viewModel::setContentId,
                            onTitleChange = viewModel::setTitle,
                            onStart = viewModel::startPack,
                        )
                    }
                    if (wide) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(16.dp),
                        ) {
                            extract(Modifier.weight(1f))
                            pack(Modifier.weight(1f))
                        }
                    } else {
                        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                            extract(Modifier.fillMaxWidth())
                            pack(Modifier.fillMaxWidth())
                        }
                    }
                }
            }
            item(key = "history") {
                HistoryCard(
                    history = state.history,
                    onClear = viewModel::clearHistory,
                )
            }
            item(key = "footer") {
                Footer()
            }
        }
    }
}

@Composable
private fun Hero() {
    Surface(
        color = MaterialTheme.colorScheme.primary,
        contentColor = MaterialTheme.colorScheme.onPrimary,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(22.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Surface(
                    color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.16f),
                    shape = CircleShape,
                ) {
                    Icon(
                        imageVector = Icons.Rounded.Archive,
                        contentDescription = null,
                        modifier = Modifier
                            .padding(10.dp)
                            .size(28.dp),
                    )
                }
                Spacer(Modifier.width(12.dp))
                Text(
                    text = stringResource(R.string.app_name),
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                )
            }
            Text(
                text = stringResource(R.string.app_tagline),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.86f),
            )
        }
    }
}

@Composable
private fun ExtractCard(
    state: com.deivid.pkgtool.UiState,
    running: Boolean,
    modifier: Modifier = Modifier,
    onPickPkg: () -> Unit,
    onPickDestination: () -> Unit,
    onStart: () -> Unit,
) {
    Card(
        modifier = modifier.animateContentSize(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            SectionHeader(
                icon = Icons.Rounded.Download,
                title = stringResource(R.string.extract_title),
                description = stringResource(R.string.extract_desc),
            )
            SelectionRow(
                icon = Icons.Rounded.InsertDriveFile,
                label = stringResource(R.string.select_pkg),
                value = state.pkgName,
                onClick = onPickPkg,
            )
            SelectionRow(
                icon = Icons.Rounded.FolderOpen,
                label = stringResource(R.string.select_dest_folder),
                value = state.extractDestName,
                onClick = onPickDestination,
            )

            val info = state.pkgInfo
            when {
                state.pkgLoading -> Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(22.dp), strokeWidth = 2.5.dp)
                    Text(
                        text = stringResource(R.string.reading_pkg),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }

                info != null -> PkgInfoBlock(
                    info = info,
                    warning = state.pkgErrorMessage,
                )

                state.pkgErrorMessage != null -> Text(
                    text = localizeError(
                        LocalContext.current,
                        state.pkgErrorCode ?: "IO",
                        state.pkgErrorMessage,
                    ),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            Button(
                onClick = onStart,
                enabled = !running && state.pkgUri != null && state.extractDestUri != null,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) {
                Icon(Icons.Rounded.Download, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.start_extract))
            }
        }
    }
}

@Composable
private fun PackCard(
    state: com.deivid.pkgtool.UiState,
    running: Boolean,
    modifier: Modifier = Modifier,
    onPickSource: () -> Unit,
    onPickOutput: () -> Unit,
    onContentIdChange: (String) -> Unit,
    onTitleChange: (String) -> Unit,
    onStart: () -> Unit,
) {
    var advanced by rememberSaveable { mutableStateOf(false) }

    Card(
        modifier = modifier.animateContentSize(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            SectionHeader(
                icon = Icons.Rounded.UploadFile,
                title = stringResource(R.string.pack_title),
                description = stringResource(R.string.pack_desc),
            )
            SelectionRow(
                icon = Icons.Rounded.FolderOpen,
                label = stringResource(R.string.select_source_folder),
                value = state.packSourceName,
                onClick = onPickSource,
            )
            SelectionRow(
                icon = Icons.Rounded.Save,
                label = stringResource(R.string.select_output_file),
                value = state.packOutputName,
                onClick = onPickOutput,
            )

            TextButton(onClick = { advanced = !advanced }) {
                Icon(
                    imageVector = if (advanced) Icons.Rounded.Close else Icons.Rounded.CreateNewFolder,
                    contentDescription = null,
                    modifier = Modifier.size(18.dp),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    stringResource(
                        if (advanced) R.string.options_hide else R.string.options_show,
                    ),
                )
            }

            AnimatedVisibility(visible = advanced) {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedTextField(
                        value = state.contentId,
                        onValueChange = onContentIdChange,
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        label = { Text(stringResource(R.string.content_id_label)) },
                        placeholder = { Text(stringResource(R.string.content_id_hint)) },
                        supportingText = { Text(stringResource(R.string.content_id_support)) },
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    )
                    OutlinedTextField(
                        value = state.title,
                        onValueChange = onTitleChange,
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        label = { Text(stringResource(R.string.title_label)) },
                        placeholder = { Text(stringResource(R.string.title_hint)) },
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    )
                }
            }

            Button(
                onClick = onStart,
                enabled = !running && state.packSourceUri != null && state.packOutputUri != null,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) {
                Icon(Icons.Rounded.UploadFile, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.start_pack))
            }
        }
    }
}

@Composable
private fun SectionHeader(icon: ImageVector, title: String, description: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Surface(
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            shape = CircleShape,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                modifier = Modifier
                    .padding(10.dp)
                    .size(26.dp),
            )
        }
        Spacer(Modifier.width(12.dp))
        Column {
            Text(
                text = title,
                style = MaterialTheme.typography.titleLarge,
            )
            Text(
                text = description,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun SelectionRow(
    icon: ImageVector,
    label: String,
    value: String,
    onClick: () -> Unit,
) {
    val shape = MaterialTheme.shapes.large
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .clickable(onClick = onClick),
        shape = shape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = value.ifBlank { stringResource(R.string.no_selection) },
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Icon(
                imageVector = Icons.Rounded.FolderOpen,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun PkgInfoBlock(
    info: com.deivid.pkgtool.core.AnalyzeInfo,
    warning: String?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(
            text = info.title.ifBlank { stringResource(R.string.info_untitled) },
            style = MaterialTheme.typography.titleMedium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        InfoLine(stringResource(R.string.info_content_id), info.contentId.ifBlank { "—" })
        InfoLine(
            stringResource(R.string.info_size),
            Formatter.formatShortFileSize(LocalContext.current, info.pkgSize),
        )
        InfoLine(
            stringResource(R.string.info_files),
            stringResource(R.string.file_count, info.fileCount),
        )
        InfoLine(stringResource(R.string.info_type), info.contentType)
        if (info.encrypted) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    Icons.Rounded.Lock,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.error,
                    modifier = Modifier.size(18.dp),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    text = stringResource(R.string.info_warning_encrypted),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        } else if (!warning.isNullOrBlank()) {
            Text(
                text = warning,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun InfoLine(label: String, value: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(96.dp),
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun JobCard(
    job: JobUiState,
    onCancel: () -> Unit,
    onDismiss: () -> Unit,
    onOpen: (JobResult) -> Unit,
    onError: (String) -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = when {
                job.error != null -> MaterialTheme.colorScheme.errorContainer
                job.result != null -> MaterialTheme.colorScheme.primaryContainer
                else -> MaterialTheme.colorScheme.surfaceContainerHighest
            },
        ),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val result = job.result
            val error = job.error
            when {
                job.running -> {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(modifier = Modifier.size(26.dp), strokeWidth = 3.dp)
                        Spacer(Modifier.width(10.dp))
                        Text(
                            text = stringResource(
                                if (job.kind == JobKind.PACK) R.string.progress_packing
                                else R.string.progress_extracting,
                            ),
                            style = MaterialTheme.typography.titleMedium,
                        )
                    }
                    WavyProgress(
                        progress = if (job.indeterminate) null else job.progress / 100f,
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(12.dp),
                    )
                    Text(
                        text = job.message,
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = if (job.indeterminate) "…" else "${job.progress}%",
                            style = MaterialTheme.typography.labelLarge,
                        )
                        OutlinedButton(onClick = onCancel) {
                            Icon(Icons.Rounded.Cancel, contentDescription = null)
                            Spacer(Modifier.width(6.dp))
                            Text(stringResource(R.string.cancel))
                        }
                    }
                }

                result != null -> {
                    JobResultBlock(
                        result = result,
                        onOpen = onOpen,
                        onDismiss = onDismiss,
                    )
                }

                error != null -> {
                    JobErrorBlock(
                        error = error,
                        onDismiss = onDismiss,
                        onError = onError,
                    )
                }
            }
        }
    }
}

@Composable
private fun JobResultBlock(
    result: JobResult,
    onOpen: (JobResult) -> Unit,
    onDismiss: () -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            imageVector = Icons.Rounded.CheckCircle,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = stringResource(R.string.result_success),
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                text = result.message,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(onClick = onDismiss) {
            Icon(Icons.Rounded.Close, contentDescription = stringResource(R.string.dismiss))
        }
    }
    Text(
        text = result.label,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
    TextButton(onClick = { onOpen(result) }) {
        Icon(Icons.Rounded.FolderOpen, contentDescription = null)
        Spacer(Modifier.width(6.dp))
        Text(stringResource(R.string.open_result))
    }
}

@Composable
private fun JobErrorBlock(
    error: JobError,
    onDismiss: () -> Unit,
    onError: (String) -> Unit,
) {
    val message = localizeError(LocalContext.current, error.code, error.message)
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            imageVector = Icons.Rounded.ErrorOutline,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.error,
        )
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = stringResource(R.string.result_error),
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(onClick = onDismiss) {
            Icon(Icons.Rounded.Close, contentDescription = stringResource(R.string.dismiss))
        }
    }
    TextButton(onClick = { onError(message) }) {
        Icon(Icons.Rounded.Info, contentDescription = null)
        Spacer(Modifier.width(6.dp))
        Text(stringResource(R.string.error_details))
    }
}

@Composable
private fun HistoryCard(
    history: List<HistoryEntry>,
    onClear: () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.History, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(
                    text = stringResource(R.string.history_title),
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.weight(1f),
                )
                if (history.isNotEmpty()) {
                    TextButton(onClick = onClear) {
                        Icon(Icons.Rounded.DeleteOutline, contentDescription = null)
                        Spacer(Modifier.width(4.dp))
                        Text(stringResource(R.string.history_clear))
                    }
                }
            }
            if (history.isEmpty()) {
                Text(
                    text = stringResource(R.string.history_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                history.forEachIndexed { index, entry ->
                    if (index > 0) {
                        HorizontalDivider()
                    }
                    HistoryRow(entry)
                }
            }
        }
    }
}

@Composable
private fun HistoryRow(entry: HistoryEntry) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = if (entry.ok) Icons.Rounded.CheckCircle else Icons.Rounded.ErrorOutline,
            contentDescription = null,
            tint = if (entry.ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
            modifier = Modifier.size(22.dp),
        )
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = entry.label,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = entry.detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Text(
            text = DateFormat
                .getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT)
                .format(Date(entry.timestamp)),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Footer() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                Icons.Rounded.Memory,
                contentDescription = null,
                modifier = Modifier.size(16.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.width(6.dp))
            Text(
                text = stringResource(R.string.footer_version, BuildConfig.VERSION_NAME),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(
            text = stringResource(R.string.unsupported_note),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun AboutDialog(onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.about_title)) },
        text = { Text(stringResource(R.string.about_body)) },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(R.string.close))
            }
        },
    )
}

private fun openResult(context: Context, result: JobResult) {
    val uri = result.outputUri?.let(Uri::parse) ?: return
    val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(
            uri,
            if (result.kind == JobKind.PACK) "application/octet-stream"
            else "vnd.android.document/directory",
        )
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    runCatching { context.startActivity(intent) }
        .onFailure {
            Toast.makeText(
                context,
                context.getString(R.string.open_failed),
                Toast.LENGTH_LONG,
            ).show()
        }
}

@Composable
private fun WavyProgress(
    progress: Float?,
    modifier: Modifier = Modifier,
) {
    val transition = rememberInfiniteTransition(label = "wavy")
    val phase by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1200, easing = LinearEasing)),
        label = "wavyPhase",
    )
    val settled by animateFloatAsState(
        targetValue = progress ?: 0f,
        animationSpec = tween(300),
        label = "wavyProgress",
    )
    val track = MaterialTheme.colorScheme.surfaceVariant
    val active = MaterialTheme.colorScheme.primary
    Canvas(modifier = modifier) {
        val stroke = 3.dp.toPx()
        val amplitude = size.height * 0.30f
        val wavelength = 22.dp.toPx().coerceAtLeast(1f)
        val centerY = size.height / 2f
        val step = 2.dp.toPx().coerceAtLeast(1f)
        fun wave(start: Float, end: Float): Path {
            val path = Path()
            var x = start
            var first = true
            while (x <= end) {
                val y = centerY + sin((x / wavelength) * (2f * PI.toFloat()) + phase * (2f * PI.toFloat())) * amplitude
                if (first) {
                    path.moveTo(x, y)
                    first = false
                } else {
                    path.lineTo(x, y)
                }
                x += step
            }
            return path
        }
        drawPath(
            path = wave(0f, size.width),
            color = track,
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        if (progress == null) {
            val segment = size.width * 0.28f
            val offset = ((phase * 1.35f) % 1f) * (size.width + segment) - segment
            drawPath(
                path = wave(offset.coerceAtLeast(0f), (offset + segment).coerceAtMost(size.width)),
                color = active,
                style = Stroke(width = stroke, cap = StrokeCap.Round),
            )
        } else if (settled > 0f) {
            drawPath(
                path = wave(0f, size.width * settled.coerceIn(0f, 1f)),
                color = active,
                style = Stroke(width = stroke, cap = StrokeCap.Round),
            )
        }
    }
}
