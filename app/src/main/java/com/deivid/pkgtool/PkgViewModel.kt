package com.deivid.pkgtool

import android.app.Application
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.deivid.pkgtool.core.AnalyzeInfo
import com.deivid.pkgtool.core.NativeCore
import com.deivid.pkgtool.core.NativeJson
import com.deivid.pkgtool.core.PkgException
import com.deivid.pkgtool.data.HistoryEntry
import com.deivid.pkgtool.data.HistoryStore
import com.deivid.pkgtool.service.PkgService
import com.deivid.pkgtool.service.documentName
import com.deivid.pkgtool.service.persistTreePermission
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class UiState(
    val pkgUri: Uri? = null,
    val pkgName: String = "",
    val pkgInfo: AnalyzeInfo? = null,
    val pkgLoading: Boolean = false,
    val pkgErrorCode: String? = null,
    val pkgErrorMessage: String? = null,
    val extractDestUri: Uri? = null,
    val extractDestName: String = "",
    val packSourceUri: Uri? = null,
    val packSourceName: String = "",
    val packOutputUri: Uri? = null,
    val packOutputName: String = "",
    val contentId: String = "",
    val title: String = "",
    val history: List<HistoryEntry> = emptyList(),
    val snackbar: String? = null,
)

class PkgViewModel(application: Application) : AndroidViewModel(application) {
    private val historyStore = HistoryStore(application)
    private val _state = MutableStateFlow(UiState(history = historyStore.list()))
    val state: StateFlow<UiState> = _state.asStateFlow()

    fun onPkgPicked(uri: Uri) {
        val name = documentName(getApplication(), uri)
        _state.update {
            it.copy(
                pkgUri = uri,
                pkgName = name,
                pkgInfo = null,
                pkgLoading = true,
                pkgErrorCode = null,
                pkgErrorMessage = null,
            )
        }
        viewModelScope.launch(Dispatchers.IO) {
            val result = runCatching {
                val descriptor = getApplication<Application>().contentResolver
                    .openFileDescriptor(uri, "r")
                    ?: throw PkgException("IO", "Não foi possível abrir o PKG selecionado")
                descriptor.use { pfd ->
                    NativeJson.parseAnalyze(NativeCore.analyzeFd(pfd.fd))
                }
            }
            result.onSuccess { info ->
                _state.update {
                    it.copy(
                        pkgLoading = false,
                        pkgInfo = info,
                        pkgErrorCode = null,
                        pkgErrorMessage = info.warning.takeIf { it.isNotBlank() },
                    )
                }
            }.onFailure { error ->
                val code = (error as? PkgException)?.code ?: "IO"
                _state.update {
                    it.copy(
                        pkgLoading = false,
                        pkgInfo = null,
                        pkgErrorCode = code,
                        pkgErrorMessage = error.message ?: "Falha ao ler o PKG",
                    )
                }
            }
        }
    }

    fun onExtractDestinationPicked(uri: Uri) {
        persistTreePermission(getApplication(), uri)
        _state.update {
            it.copy(
                extractDestUri = uri,
                extractDestName = documentName(getApplication(), uri),
            )
        }
    }

    fun onPackSourcePicked(uri: Uri) {
        persistTreePermission(getApplication(), uri)
        _state.update {
            it.copy(
                packSourceUri = uri,
                packSourceName = documentName(getApplication(), uri),
            )
        }
    }

    fun onPackOutputPicked(uri: Uri) {
        _state.update {
            it.copy(
                packOutputUri = uri,
                packOutputName = documentName(getApplication(), uri),
            )
        }
    }

    fun setContentId(value: String) {
        _state.update { it.copy(contentId = value) }
    }

    fun setTitle(value: String) {
        _state.update { it.copy(title = value) }
    }

    fun startExtract() {
        val current = _state.value
        if (current.pkgUri == null || current.extractDestUri == null) {
            showSnackbar("Selecione o PKG e a pasta de destino")
            return
        }
        PkgService.startExtract(
            getApplication(),
            current.pkgUri,
            current.extractDestUri,
            current.pkgName.ifBlank { "game.pkg" },
        )
    }

    fun startPack() {
        val current = _state.value
        if (current.packSourceUri == null || current.packOutputUri == null) {
            showSnackbar("Selecione a pasta de origem e onde salvar o PKG")
            return
        }
        PkgService.startPack(
            getApplication(),
            current.packSourceUri,
            current.packOutputUri,
            current.contentId.trim(),
            current.title.trim(),
            current.packOutputName.ifBlank { "output.pkg" },
        )
    }

    fun cancelJob() {
        PkgService.cancel(getApplication())
    }

    fun refreshHistory() {
        _state.update { it.copy(history = historyStore.list()) }
    }

    fun clearHistory() {
        historyStore.clear()
        refreshHistory()
    }

    fun showSnackbar(message: String) {
        _state.update { it.copy(snackbar = message) }
    }

    fun clearSnackbar() {
        _state.update { it.copy(snackbar = null) }
    }

    fun addHistoryForInterruptedJob() {
        // Chamado quando a UI detecta que havia um resultado e o processo foi
        // recriado sem serviço ativo. Mantido simples: apenas atualiza a lista.
        refreshHistory()
    }
}
