package com.deivid.pkgtool.service

import android.content.Context
import com.deivid.pkgtool.data.JobKind

/**
 * Marca uma operação em andamento no disco. Se o processo for morto pelo
 * sistema, o marcador permanece e a próxima abertura da UI pode avisar que a
 * operação foi interrompida em vez de fingir que nada aconteceu.
 */
object JobMarker {
    private const val PREFS = "pkgtool_job_marker"
    private const val KEY_KIND = "kind"

    fun mark(context: Context, kind: JobKind) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_KIND, kind.name)
            .commit()
    }

    fun clear(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .remove(KEY_KIND)
            .commit()
    }

    fun take(context: Context): JobKind? {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val value = prefs.getString(KEY_KIND, null) ?: return null
        prefs.edit().remove(KEY_KIND).commit()
        return runCatching { JobKind.valueOf(value) }.getOrNull()
    }
}
