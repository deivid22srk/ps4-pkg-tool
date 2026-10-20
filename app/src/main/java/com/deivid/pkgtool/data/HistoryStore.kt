package com.deivid.pkgtool.data

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

enum class JobKind { EXTRACT, PACK }

data class HistoryEntry(
    val kind: JobKind,
    val ok: Boolean,
    val label: String,
    val detail: String,
    val timestamp: Long,
    val durationMs: Long,
)

class HistoryStore(context: Context) {
    private val prefs = context.getSharedPreferences("pkgtool_history", Context.MODE_PRIVATE)

    fun list(): List<HistoryEntry> {
        val raw = prefs.getString(KEY, null) ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            buildList {
                for (i in 0 until array.length()) {
                    val obj = array.getJSONObject(i)
                    add(
                        HistoryEntry(
                            kind = runCatching { JobKind.valueOf(obj.optString("kind")) }
                                .getOrDefault(JobKind.EXTRACT),
                            ok = obj.optBoolean("ok"),
                            label = obj.optString("label"),
                            detail = obj.optString("detail"),
                            timestamp = obj.optLong("timestamp"),
                            durationMs = obj.optLong("durationMs"),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    fun add(entry: HistoryEntry) {
        val entries = list().toMutableList()
        entries.add(0, entry)
        while (entries.size > MAX) {
            entries.removeAt(entries.lastIndex)
        }
        val array = JSONArray()
        for (item in entries) {
            array.put(
                JSONObject().apply {
                    put("kind", item.kind.name)
                    put("ok", item.ok)
                    put("label", item.label)
                    put("detail", item.detail)
                    put("timestamp", item.timestamp)
                    put("durationMs", item.durationMs)
                },
            )
        }
        prefs.edit().putString(KEY, array.toString()).apply()
    }

    fun clear() {
        prefs.edit().remove(KEY).apply()
    }

    private companion object {
        const val KEY = "entries"
        const val MAX = 12
    }
}
