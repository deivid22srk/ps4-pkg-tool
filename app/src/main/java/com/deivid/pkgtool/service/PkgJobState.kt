package com.deivid.pkgtool.service

import com.deivid.pkgtool.data.JobKind
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update

data class JobResult(
    val kind: JobKind,
    val message: String,
    val outputUri: String?,
    val label: String,
    val fileCount: Long,
    val bytes: Long,
    val durationMs: Long,
)

data class JobError(
    val kind: JobKind?,
    val code: String,
    val message: String,
)

data class JobUiState(
    val running: Boolean = false,
    val kind: JobKind? = null,
    val progress: Int = 0,
    val indeterminate: Boolean = false,
    val message: String = "",
    val startedAt: Long = 0L,
    val result: JobResult? = null,
    val error: JobError? = null,
)

/**
 * Estado compartilhado entre o Foreground Service e a UI. Como o serviço roda
 * no mesmo processo, o StateFlow sobrevive à rotação e ao app ir para segundo
 * plano.
 */
object PkgJobState {
    val state = MutableStateFlow(JobUiState())

    fun begin(kind: JobKind, message: String) {
        state.value = JobUiState(
            running = true,
            kind = kind,
            progress = 0,
            indeterminate = true,
            message = message,
            startedAt = System.currentTimeMillis(),
        )
    }

    fun progress(percent: Int, message: String) {
        state.update {
            it.copy(
                running = true,
                progress = percent.coerceIn(0, 100),
                indeterminate = percent < 0,
                message = message,
            )
        }
    }

    fun success(result: JobResult) {
        state.update {
            it.copy(
                running = false,
                progress = 100,
                indeterminate = false,
                message = result.message,
                result = result,
                error = null,
            )
        }
    }

    fun failure(error: JobError) {
        state.update {
            it.copy(
                running = false,
                indeterminate = false,
                message = error.message,
                result = null,
                error = error,
            )
        }
    }

    fun dismiss() {
        state.value = JobUiState()
    }
}
