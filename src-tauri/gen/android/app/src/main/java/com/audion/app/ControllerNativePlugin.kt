package com.audion.app

import android.app.Activity
import android.content.Intent
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class CredentialArgs {
    lateinit var hostId: String
    var bytes: ByteArray = byteArrayOf()
}

/**
 * Native JNI dispatch only. This is not a JavascriptInterface. The paired Rust
 * plugin rejects every WebView invocation unconditionally (including if ACL is
 * misgranted), returns true to prevent mobile fallback, and privately owns the
 * PluginHandle used for these calls. No frontend capability grants access.
 */
@TauriPlugin
class ControllerNativePlugin(private val activity: Activity) : Plugin(activity) {
    private val secrets by lazy { ControllerSecrets(activity.applicationContext) }

    @Command fun saveCredentials(invoke: Invoke) {
        var bytes: ByteArray? = null
        try {
            val args = invoke.parseArgs(CredentialArgs::class.java)
            bytes = args.bytes
            secrets.saveCredentials(args.hostId, args.bytes)
            invoke.resolve()
        } catch (_: Exception) {
            invoke.reject("Secure storage is unavailable. Pair again.")
        } finally { bytes?.fill(0) }
    }
    @Command fun loadCredentials(invoke: Invoke) {
        var bytes: ByteArray? = null
        try {
            val args = invoke.parseArgs(CredentialArgs::class.java)
            bytes = secrets.loadCredentials(args.hostId)
            val result = JSObject()
            result.put("bytes", bytes?.let { JSArray(it.map { byte -> byte.toInt() and 255 }) } ?: JSONObject.NULL)
            invoke.resolve(result)
        } catch (_: Exception) {
            invoke.reject("Secure storage is unavailable. Pair again.")
        } finally { bytes?.fill(0) }
    }
    @Command fun deleteCredentials(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CredentialArgs::class.java)
            secrets.deleteCredentials(args.hostId)
            invoke.resolve()
        } catch (_: Exception) {
            invoke.reject("Secure storage is unavailable. Pair again.")
        }
    }
    @Command fun scanInvitation(invoke: Invoke) {
        startActivityForResult(invoke, Intent(activity, ControllerCaptureActivity::class.java), "invitationResult")
    }
    @ActivityCallback fun invitationResult(invoke: Invoke, result: ActivityResult) {
        val invitation = if (result.resultCode == Activity.RESULT_OK) result.data?.getStringExtra("invitation") else null
        val response = JSObject()
        response.put("invitation", invitation ?: JSONObject.NULL)
        invoke.resolve(response)
    }
}
