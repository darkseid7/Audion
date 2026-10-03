package com.audion.app

import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import java.security.KeyStore
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/** Device-only acceptance: never substitute JVM crypto results for Keystore proof. */
@RunWith(AndroidJUnit4::class)
class ControllerSecretsTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val host = "cf8dd70c-8cc2-4640-bd40-5b06f68cc301"
    private val otherHost = "cf8dd70c-8cc2-4640-bd40-5b06f68cc302"
    private val secret = ByteArray(64) { (it + 1).toByte() }
    private val directory get() = File(context.noBackupFilesDir, "controller-credentials-v1")
    private val store get() = ControllerSecrets(context)
    @Before fun resetFixtureHosts() {
        store.deleteCredentials(host)
        store.deleteCredentials(otherHost)
    }
    @Test fun roundTrip() {
        store.saveCredentials(host, secret)
        assertArrayEquals(secret, store.loadCredentials(host))
    }
    @Test fun tamperedEnvelopeRequiresPairing() {
        for (offset in listOf(0, 4, 16, 32)) {
            store.saveCredentials(host, secret)
            val file = File(directory, "$host.bin")
            val bytes = file.readBytes()
            bytes[offset] = (bytes[offset].toInt() xor 1).toByte()
            file.writeBytes(bytes)
            assertNull(store.loadCredentials(host))
        }
    }
    @Test fun crossHostDecryptRejected() {
        store.saveCredentials(host, secret)
        store.saveCredentials(otherHost, secret)
        File(directory, "$host.bin").copyTo(File(directory, "$otherHost.bin"), overwrite = true)
        assertNull(store.loadCredentials(otherHost))
        assertArrayEquals(secret, store.loadCredentials(host))
    }
    @Test fun missingKeyRequiresPairing() {
        store.saveCredentials(host, secret)
        KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            .deleteEntry("audion.controller.credentials.v1")
        assertNull(store.loadCredentials(host))
    }
}
