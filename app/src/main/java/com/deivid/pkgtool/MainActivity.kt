package com.deivid.pkgtool

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import com.deivid.pkgtool.ui.PkgToolApp
import com.deivid.pkgtool.ui.theme.PkgToolTheme

class MainActivity : ComponentActivity() {
    private val viewModel: PkgViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            PkgToolTheme {
                PkgToolApp(viewModel = viewModel)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
    }

    companion object {
        const val EXTRA_OPEN_URI = "open_uri"
    }
}
