package com.audion.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions

/** Optional camera UI. Scan and paste return the same opaque, bounded input. */
class ControllerCaptureActivity : AppCompatActivity() {
    private lateinit var input: EditText
    private lateinit var status: TextView
    private val scan = registerForActivityResult(ScanContract()) { result ->
        result.contents?.let { finishWithInvitation(it) }
            ?: run { status.text = "Scan cancelled. You can paste the invitation instead." }
    }
    private val cameraPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) launchScan()
        else status.text = "Camera permission denied. Paste the invitation instead."
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val layout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(24, 24, 24, 24) }
        status = TextView(this).apply { text = "Scan the PC invitation or paste it below." }
        input = EditText(this).apply {
            hint = "Paste invitation"
            setText(savedInstanceState?.getString("invitation"))
            setSelectAllOnFocus(true)
        }
        layout.addView(status)
        layout.addView(input)
        fun button(label: String, action: () -> Unit) {
            layout.addView(Button(this).apply { text = label; setOnClickListener { action() } })
        }
        button("Scan QR") {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) launchScan()
            else cameraPermission.launch(Manifest.permission.CAMERA)
        }
        button("Use invitation") { finishWithInvitation(input.text.toString()) }
        button("Cancel") { setResult(Activity.RESULT_CANCELED); finish() }
        setContentView(layout)
    }
    private fun launchScan() {
        scan.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setOrientationLocked(false).setBeepEnabled(false).setBarcodeImageEnabled(false)
            .setPrompt("Scan the invitation displayed on your PC"))
    }
    private fun finishWithInvitation(text: String) {
        if (text.isBlank() || text.toByteArray(Charsets.UTF_8).size > 2048) {
            status.text = "The invitation is empty or too large. Copy a new invitation from the PC."
            return
        }
        setResult(Activity.RESULT_OK, Intent().putExtra("invitation", text))
        finish()
    }
    override fun onSaveInstanceState(outState: Bundle) {
        outState.putString("invitation", input.text.toString())
        super.onSaveInstanceState(outState)
    }
}
