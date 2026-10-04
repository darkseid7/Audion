package com.audion.app

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback

class MainActivity : TauriActivity() {
    private val backBridge = ControllerBackBridge { moveTaskToBack(true) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() { backBridge.back() }
        })
    }

    override fun onWebViewCreate(webView: WebView) {
        super.onWebViewCreate(webView)
        backBridge.attach { script, reply -> webView.evaluateJavascript(script) { reply(it) } }
    }

    override fun onDestroy() {
        backBridge.destroy()
        super.onDestroy()
    }
}
